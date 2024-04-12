use crate::cap_std_capnp::file;
use crate::posix_spawn_capnp::local_native_program;
use crate::spawn::posix_process::PosixProgramImpl;
use capnp_macros::capnproto_rpc;

use crate::posix_spawn_capnp::{posix_args::Owned as PosixArgs, posix_error::Owned as PosixError};

use crate::byte_stream_capnp::byte_stream::Owned as ByteStream;
type PosixProcessClient = crate::spawn_capnp::program::Client<PosixArgs, ByteStream, PosixError>;

pub struct LocalNativeProgramImpl {}

#[capnproto_rpc(local_native_program)]
impl local_native_program::Server for LocalNativeProgramImpl {
    fn file(&mut self, file: Client) {
        Ok(async move {
            match file.raw_handle_request().send().promise.await {
                Ok(h) => {
                    let handle = h.get().unwrap().get_handle();
                    let program = PosixProgramImpl::new(handle);

                    let program_client: PosixProcessClient = capnp_rpc::new_client(program);
                    results.get().set_result(program_client);
                    Ok(())
                }
                Err(e) => Err(capnp::Error::failed(e.to_string())),
            }
        })
    }
}
