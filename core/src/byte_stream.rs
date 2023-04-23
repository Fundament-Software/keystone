use bytes::BytesMut;
use capnp::{capability::{Promise, Response}, ErrorKind};
use capnp_rpc::pry;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use futures::{future::FutureExt, TryFutureExt};

use crate::stream_capnp::stream_result;
use crate::byte_stream_capnp::byte_stream::{Client, Server, WriteParams, WriteResults, EndParams, EndResults};

/// Service-side implementation of a ByteStream capability that uses a function consumer.
///
/// In Keystone, bytes streams work through RPC, by providing the client with a capability
/// which they can call the `write` method on with arbitrary bytes. The server responds with
/// an empty promise which will be resolved when the server is ready to recieve more bytes.
///
/// ConsumerByteStream is constructed with a consumer function `C` which will consume the bytes recieved by 
/// rpc `write` calls and return a promise which resolves when the system is ready to process
/// more bytes.
pub struct ConsumerByteStream<C> {
    consumer: C,
    closed: bool
}

impl<C> ConsumerByteStream<C>
where
    C: FnMut(&[u8]) -> Promise<(), capnp::Error>
{
    pub fn new(consumer: C) -> Self {
        Self {
            consumer,
            closed: false
        }
    }
}

impl<C> Server for ConsumerByteStream<C>
where
    C: FnMut(&[u8]) -> Promise<(), capnp::Error>
{
    fn write(&mut self, params: WriteParams, mut _results: WriteResults) ->  Promise<(), capnp::Error> {
        if self.closed {
            return Promise::err(capnp::Error { 
                kind: ErrorKind::Failed,
                description: String::from("Write called on byte stream after closed.")
            });
        }

        let byte_reader = pry!(params.get());
        let bytes = pry!(byte_reader.get_bytes());

        (self.consumer)(bytes)
    } 

    fn end(&mut self, _: EndParams, _: EndResults<>) -> Promise<(), capnp::Error> {
        self.closed = true;
        Promise::ok(())
    }

    fn get_substream(&mut self, _:crate::byte_stream_capnp::byte_stream::GetSubstreamParams<>,_:crate::byte_stream_capnp::byte_stream::GetSubstreamResults<>) ->  capnp::capability::Promise<(), capnp::Error> {
        Promise::err(capnp::Error { kind: ErrorKind::Unimplemented, description: String::from("Not implemented") })
    }
}

/// Service-side implementation of a ByteStream capability that uses an [AsyncWrite] consumer.
pub struct AsyncWriteByteStream<W> (W);

impl<W> AsyncWriteByteStream<W>
where W: AsyncWrite + Unpin
{
    pub fn new(async_write: W) -> Self {
        Self(async_write)
    }
}

impl<W> Server for AsyncWriteByteStream<W>
where W: AsyncWrite + Unpin
{
    fn write(&mut self, params: WriteParams, _: WriteResults) -> Promise<(), capnp::Error> {
        let byte_reader = pry!(params.get());
        let bytes = pry!(byte_reader.get_bytes());

        let f = self.0.write_all(bytes).map_err(|e| capnp::Error::failed(e.to_string()));
        Promise::from_future(f)
    }

    fn end(&mut self, _: EndParams, _: EndResults) -> Promise<(), capnp::Error> {
        Promise::from_future(self.0.shutdown().map_err(|e| capnp::Error::failed(e.to_string())))
    }
}

impl Client {
    /// Convenience function to make it easier to send bytes through the ByteStream
    pub async fn write_bytes(&self, bytes: &[u8]) -> Result<Response<stream_result::Owned>, capnp::Error> {
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
    pub async fn copy(&self, reader: &mut (impl AsyncRead + Unpin)) -> anyhow::Result<usize> {
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

#[test]
fn write_test() -> anyhow::Result<()> {
    let server = ConsumerByteStream::new(|bytes| {
        assert_eq!(bytes, &[73, 22, 66, 91]);
        Promise::ok(())
    });

    let client: crate::byte_stream_capnp::byte_stream::Client = capnp_rpc::new_client(server);
    let mut write_request = client.write_request();
    write_request.get().set_bytes(&[73, 22, 66, 91]);
    
    let write_result = futures::executor::block_on(write_request.send().promise);
    let _ = write_result.unwrap(); // Ensure that server didn't return an error

    anyhow::Ok(())
}
