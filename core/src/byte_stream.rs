use capnp::{capability::{Promise, Response}, ErrorKind};
use capnp_rpc::pry;

use crate::stream_capnp::stream_result;
use crate::byte_stream_capnp::byte_stream::{Client, Server, WriteParams, WriteResults, EndParams, EndResults};

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
    consumer: C,
    closed: bool
}

impl<C> ByteStreamImpl<C>
where
    C: FnMut(&[u8]) -> Promise<(), capnp::Error>
{
    fn new(consumer: C) -> Self {
        Self {
            consumer,
            closed: false
        }
    }
}

impl<C> Server for ByteStreamImpl<C>
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

impl Client {
    /// Convinience function to make it easier to send bytes through the ByteStream
    async fn write_bytes(&self, bytes: &[u8]) -> Result<Response<stream_result::Owned>, capnp::Error> {
        let mut write_request = self.write_request();
        write_request.get().set_bytes(bytes);
        write_request.send().promise.await
    }
}

#[test]
fn write_test() -> anyhow::Result<()> {
    let server = ByteStreamImpl::new(|bytes| {
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
