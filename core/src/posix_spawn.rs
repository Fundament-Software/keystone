use crate::cap_std_capnproto::AmbientAuthorityImpl;
use crate::posix_process::PosixProgramImpl;
use crate::posix_spawn_capnp::local_native_program;
use capnp_macros::capnproto_rpc;
use std::cell::RefCell;
use std::rc::Rc;

use crate::posix_spawn_capnp::{posix_args::Owned as PosixArgs, posix_error::Owned as PosixError};

use crate::byte_stream_capnp::byte_stream::Owned as ByteStream;
type PosixProcessClient = crate::spawn_capnp::program::Client<PosixArgs, ByteStream, PosixError>;

pub struct LocalNativeProgramImpl {
    auth_ref: Rc<RefCell<AmbientAuthorityImpl>>,
    process_set: Rc<RefCell<crate::posix_process::ProcessCapSet>>,
}

#[capnproto_rpc(local_native_program)]
impl local_native_program::Server for LocalNativeProgramImpl {
    async fn file(self: Rc<Self>, file: Client) {
        let span = tracing::span!(tracing::Level::DEBUG, "LocalNativeProgram::file");
        let _enter = span.enter();
        if let Some(handle) = self.auth_ref.borrow().get_file_handle(&file).await {
            let program =
                PosixProgramImpl::new(handle, self.process_set.clone(), "warn".to_string());

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
