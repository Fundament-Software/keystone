use crate::cap_std_capnp::file;
use crate::posix_spawn_capnp::local_native_program;
use crate::spawn::posix_process::PosixProgramImpl;
use capnp_macros::{capnp_let, capnproto_rpc};

use crate::posix_spawn_capnp::{posix_args::Owned as PosixArgs, posix_error::Owned as PosixError};

use crate::byte_stream_capnp::{
    byte_stream::Client as ByteStreamClient, byte_stream::Owned as ByteStream,
};
type PosixProcessClient = crate::spawn_capnp::program::Client<PosixArgs, ByteStream, PosixError>;

pub struct LocalNativeProgramImpl {}

#[capnproto_rpc(local_native_program)]
impl local_native_program::Server for LocalNativeProgramImpl {
    fn file(&mut self, file: Client) {
        let program = PosixProgramImpl::new("sh");

        let program_client: PosixProcessClient = capnp_rpc::new_client(program);
        //crate::spawn_capnp::program::Client<crate::posix_spawn_capnp::posix_args::Owned, crate::byte_stream_capnp::byte_stream::Owned, crate::posix_spawn_capnp::posix_error::Owned>
        results.get().set_result(program_client);
        capnp::ok()
    }
}
