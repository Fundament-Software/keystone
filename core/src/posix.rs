use crate::cap_std_capnp::dir;
use crate::capnp;
use crate::capnp::any_pointer::Owned as any_pointer;
use crate::capnp::any_pointer::Owned as cap_pointer;
use crate::capnp_rpc;
use crate::capnp_rpc::CapabilityServerSet;
use crate::keystone::CapnpResult;
use crate::module_capnp::{module_args, module_error};
use crate::posix_capnp::local_posix_program;
use crate::posix_capnp::{posix_args::Owned as PosixArgs, posix_error::Owned as PosixError};
use capnp_macros::capnproto_rpc;
use std::process::ExitStatus;
use std::rc::Rc;

use crate::byte_stream_capnp::byte_stream::{Client as ByteStreamClient, Owned as ByteStream};
type PosixProgramClient = program::Client<PosixArgs, ByteStream, PosixError>;

pub type ProgramCapSet = CapabilityServerSet<PosixProgramImpl, PosixProgramClient>;

pub struct PosixProgramImpl {
    program: cap_std::fs::File,
}

impl PosixProgramImpl {
    pub(crate) fn new(handle: u64) -> Self {
        use cap_std::io_lifetimes::raw::{FromRawFilelike, RawFilelike};
        unsafe {
            Self {
                program: cap_std::fs::File::from_raw_filelike(handle as RawFilelike),
            }
        }
    }
    pub(crate) fn new_std(file: std::fs::File) -> Self {
        Self {
            program: cap_std::fs::File::from_std(file),
        }
    }
}

use crate::spawn_capnp::program::SpawnResults;
use crate::spawn_capnp::program::{self, SpawnParams};

impl program::Server<PosixArgs, ByteStream, PosixError> for PosixProgramImpl {
    async fn spawn(
        self: Rc<Self>,
        params: SpawnParams<PosixArgs, ByteStream, PosixError>,
        mut results: SpawnResults<PosixArgs, ByteStream, PosixError>,
    ) -> capnp::Result<()> {
        let params_reader = params.get()?;

        let args = params_reader.get_args()?;
        let stdout: ByteStreamClient = args.get_stdout()?;
        let stderr: ByteStreamClient = args.get_stderr()?;
        let argv: capnp::text_list::Reader = args.get_args()?;
        let argv_iter = argv.into_iter().map(|item| match item {
            Ok(i) => Ok(i.to_str().to_capnp()?),
            Err(e) => Err(e),
        });

        let process = crate::posix_process::PosixProcessImpl::spawn_process(
            &self.program,
            argv_iter,
            "warn",
            stdout,
            stderr,
        )
        .await
        .map_err(|e| capnp::Error::failed(e.to_string()))?;

        results.get().set_result(capnp_rpc::new_client(process));
        Ok(())
    }
}

#[capnproto_rpc(local_posix_program)]
impl local_posix_program::Server for crate::KeystoneRoot {
    async fn file(self: Rc<Self>, file: Client) {
        let span = tracing::span!(tracing::Level::DEBUG, "LocalPosixProgram::file");
        let _enter = span.enter();

        let resolved = capnp::capability::get_resolved_cap(file).await;
        if let Some(handle) = self.file_server.borrow().get_file_handle(&resolved) {
            let program = PosixProgramImpl::new(handle);

            let program_client: PosixProgramClient =
                self.program_set.borrow_mut().new_client(program);
            results.get().set_result(program_client);
            Ok(())
        } else {
            Err(capnp::Error::failed(
                "File capability wasn't created by this keystone instance!".to_string(),
            ))
        }
    }
}

use crate::posix_capnp::posix_module;

#[capnproto_rpc(posix_module)]
impl posix_module::Server for crate::KeystoneRoot {
    async fn wrap(self: Rc<Self>, prog: Client) {
        let span = tracing::span!(tracing::Level::DEBUG, "posix_module::wrap");
        let _enter = span.enter();
        let internal = self
            .program_set
            .borrow_mut()
            .get_local_server_of_resolved(&prog)
            .ok_or(capnp::Error::failed(
                "Program wasn't from this keystone instance!".into(),
            ))?;

        let module_program = crate::posix_module::PosixModuleProgramImpl::new(
            internal.program.try_clone()?,
            self.file_server.clone(),
            self.id_set.clone(),
            self.db.clone(),
            self.log_tx.clone(),
        )?;

        let module_program_client: program::Client<
            module_args::Owned<any_pointer, dir::Owned>,
            cap_pointer,
            module_error::Owned<any_pointer>,
        > = capnp_rpc::new_client(module_program);

        results.get().set_result(module_program_client);
        Ok(())
    }
}

pub fn get_exit_code(status: &ExitStatus) -> i32 {
    #[cfg(not(windows))]
    use std::os::unix::process::ExitStatusExt;

    #[cfg(not(windows))]
    return status
        .code()
        .unwrap_or_else(|| v.signal().unwrap_or(i32::MIN));

    #[cfg(windows)]
    return status.code().unwrap_or(i32::MIN);
}
/*
pub fn get_file_hash(source: &cap_std::fs::File) -> std::io::Result<u64> {
    use cap_std::io_lifetimes::raw::AsRawFilelike;
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;

    let mut buf = [0_u16; 2048];

    let bytes = unsafe {
        let num = GetFinalPathNameByHandleW(source.as_raw_filelike(), buf.as_mut_ptr(), 2048, 0);
        &std::mem::transmute::<[u16; 2048], [u8; 4096]>(buf)[0..(num * 2) as usize]
    };

    #[cfg(not(target_os = "windows"))]
    let bytes = std::fs::read_link(format!(
        "/proc/{}/fd/{}",
        std::process::id(),
        source.as_raw_filelike()
    ))?
    .canonicalize()?
    .as_os_str()
    .as_encoded_bytes();

    Ok(i128_hash(caplog::murmur3::murmur3_unaligned(bytes, 273849)))
}

// Adopted from khash 64 -> 32 int hash
#[inline]
fn i128_hash(key: u128) -> u64 {
    ((key) >> 66 ^ (key) ^ (key) << 22) as u64
}
*/
