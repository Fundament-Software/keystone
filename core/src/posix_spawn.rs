use crate::cap_std_capnp::file;
use crate::cap_std_capnproto::AmbientAuthorityImpl;
use crate::posix_spawn_capnp::local_native_program;
use crate::spawn::posix_process::PosixProgramImpl;
use capnp_macros::capnproto_rpc;

use crate::posix_spawn_capnp::{posix_args::Owned as PosixArgs, posix_error::Owned as PosixError};

use crate::byte_stream_capnp::byte_stream::Owned as ByteStream;
type PosixProcessClient = crate::spawn_capnp::program::Client<PosixArgs, ByteStream, PosixError>;

pub struct LocalNativeProgramImpl<'a> {
    auth_ref: &'a AmbientAuthorityImpl,
}

#[capnproto_rpc(local_native_program)]
impl<'a> local_native_program::Server for LocalNativeProgramImpl<'a> {
    #[async_backtrace::framed]
    async fn file(&self, file: Client) {
        if let Some(handle) = self.auth_ref.get_file_handle(&file).await {
            let program = PosixProgramImpl::new(handle);

            let program_client: PosixProcessClient = capnp_rpc::new_client(program);
            results.get().set_result(program_client);
            Ok(())
        } else {
            Err(capnp::Error::failed(
                "File capability wasn't created by this keystone instance!".to_string(),
            ))
        }
    }
}
