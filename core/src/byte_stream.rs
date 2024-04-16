use bytes::BytesMut;
use capnp::{
    capability::{Promise, Response},
    ErrorKind,
};
use capnp_macros::capnproto_rpc;
use futures::AsyncWrite;
use futures::FutureExt;
use std::sync::atomic::AtomicUsize;
use std::task::Poll;
use std::task::Waker;
use std::{cell::RefCell, rc::Rc};
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::byte_stream_capnp::{
    self,
    byte_stream::{Client, Server},
};
use crate::stream_capnp::stream_result;

/// Server implementation of a ByteStream capability.
///
/// In Keystone, bytes streams work through RPC, by providing the client with a capability
/// which they can call the `write` method on with arbitrary bytes. The server responds with
/// an empty promise which will be resolved when the server is ready to recieve more bytes.
///
/// ByteStreamImpl is constructed with a Consumer `C` which will consume the bytes recieved by
/// rpc `write` calls and return a promise which resolves when the system is ready to process
/// more bytes.
pub struct ByteStreamImpl<C> {
    consumer: Option<C>,
}

impl<C> ByteStreamImpl<C>
where
    C: FnMut(&[u8]) -> Promise<(), capnp::Error>,
{
    pub fn new(consumer: C) -> Self {
        Self {
            consumer: Some(consumer),
        }
    }
}

#[capnproto_rpc(byte_stream_capnp::byte_stream)]
impl<C> Server for ByteStreamImpl<C>
where
    C: FnMut(&[u8]) -> Promise<(), capnp::Error>,
{
    fn write(&mut self, bytes: &[u8]) {
        if let Some(f) = &mut self.consumer {
            Ok((f)(bytes))
        } else {
            Err(capnp::Error {
                kind: ErrorKind::Failed,
                extra: String::from("Write called on byte stream after closed."),
            })
        }
    }

    fn end(&mut self) {
        self.consumer = None;
        capnp::ok()
    }

    fn get_substream(&mut self) {
        Ok(async {
            Err(capnp::Error {
                kind: ErrorKind::Unimplemented,
                extra: String::from("Not implemented"),
            })
        })
    }
}

struct ByteStreamBufferInternal {
    buf: Vec<u8>,
    write_waker: Option<Waker>,
    read_waker: Option<Waker>,
    pending: AtomicUsize,
}

#[derive(Clone)]
pub struct ByteStreamBufferImpl {
    internal: Rc<RefCell<ByteStreamBufferInternal>>,
    closed: bool,
}

impl ByteStreamBufferImpl {
    pub fn new() -> Self {
        Self {
            internal: Rc::new(RefCell::new(ByteStreamBufferInternal {
                buf: Vec::new(),
                write_waker: None,
                read_waker: None,
                pending: AtomicUsize::new(0),
            })),
            closed: false,
        }
    }
}

impl std::future::Future for ByteStreamBufferImpl {
    type Output = ();

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self
            .internal
            .borrow_mut()
            .pending
            .load(std::sync::atomic::Ordering::Relaxed)
            == 0
        {
            Poll::Ready(())
        } else {
            self.internal.borrow_mut().write_waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[capnproto_rpc(byte_stream_capnp::byte_stream)]
impl Server for ByteStreamBufferImpl {
    fn write(&mut self, bytes: &[u8]) {
        if !self.closed {
            let owned_bytes = bytes.to_owned();
            let r = self.clone();
            Ok(Promise::from_future(async move {
                r.clone().await;
                let mut this = r.internal.borrow_mut();
                this.buf = owned_bytes;
                this.pending
                    .store(this.buf.len(), std::sync::atomic::Ordering::Release);
                if let Some(w) = this.read_waker.take() {
                    w.wake();
                }
                Ok(())
            }))
        } else {
            Err(capnp::Error {
                kind: ErrorKind::Failed,
                extra: String::from("Write called on byte stream after closed."),
            })
        }
    }

    fn end(&mut self) {
        self.closed = true;
        capnp::ok()
    }

    fn get_substream(&mut self) {
        Ok(async {
            Err(capnp::Error {
                kind: ErrorKind::Unimplemented,
                extra: String::from("Not implemented"),
            })
        })
    }
}

impl futures::AsyncRead for ByteStreamBufferImpl {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut this = self.internal.borrow_mut();
        let pending = this.pending.load(std::sync::atomic::Ordering::Acquire);
        if pending > 0 {
            let start = this.buf.len() - pending;
            let len = std::cmp::min(pending, buf.len());
            buf[0..len].copy_from_slice(&this.buf[start..(start + len)]);
            this.pending
                .fetch_sub(len, std::sync::atomic::Ordering::Release);
            if let Some(w) = this.write_waker.take() {
                w.wake()
            }
            std::task::Poll::Ready(Ok(len))
        } else {
            this.read_waker = Some(cx.waker().clone());
            std::task::Poll::Pending
        }
    }
}

enum PassThrough<'a> {
    PendingRead(&'a [u8]),
    PendingWrite(&'a [u8]),
    Ready,
}

impl Client {
    /// Convenience function to make it easier to send bytes through the ByteStream
    #[async_backtrace::framed]
    pub async fn write_bytes(
        &self,
        bytes: &[u8],
    ) -> Result<Response<stream_result::Owned>, capnp::Error> {
        let mut write_request = self.write_request();
        write_request.get().set_bytes(bytes);
        write_request.send().promise.await
    }

    /// Copies the entire contents of a reader into the byte stream.
    ///
    /// This function returns a future that will continiously read data from `reader` and then
    /// write it to `self` in a streaming fashion until `reader` returns EOF, or an error occurs.
    ///
    /// On success, returns the total number of bytes that were copies from `reader` to the byte
    /// stream.
    ///
    /// A copy buffer of 4 KB is created to take data from the reader to the byte stream.
    #[async_backtrace::framed]
    pub async fn copy(&self, reader: &mut (impl AsyncRead + Unpin)) -> eyre::Result<usize> {
        let mut total_bytes = 0;
        let mut buffer = BytesMut::with_capacity(4096);

        loop {
            let read_size = reader.read_buf(&mut buffer).await?;
            // If we read zero bytes then EOF has been reached.
            if read_size == 0 {
                break;
            }

            total_bytes += read_size;
            self.write_bytes(&buffer[..read_size]).await?;
            buffer.clear();
        }

        Ok(total_bytes)
    }
}

impl AsyncWrite for Client {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        let mut write_request = self.write_request();
        write_request.get().set_bytes(buf);
        match write_request.send().promise.poll_unpin(cx) {
            std::task::Poll::Ready(Ok(_)) => std::task::Poll::Ready(Ok(buf.len())),
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match self.end_request().send().promise.poll_unpin(cx) {
            std::task::Poll::Ready(_) => std::task::Poll::Ready(Ok(())),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

#[test]
fn write_test() -> eyre::Result<()> {
    let server = ByteStreamImpl::new(|bytes| {
        assert_eq!(bytes, &[73, 22, 66, 91]);
        Promise::ok(())
    });

    let client: crate::byte_stream_capnp::byte_stream::Client = capnp_rpc::new_client(server);
    let mut write_request = client.write_request();
    write_request.get().set_bytes(&[73, 22, 66, 91]);

    let write_result = futures::executor::block_on(write_request.send().promise);
    let _ = write_result.unwrap(); // Ensure that server didn't return an error

    eyre::Result::Ok(())
}
