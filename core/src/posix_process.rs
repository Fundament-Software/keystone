use crate::keystone::CapnpResult;

#[cfg(not(windows))]
pub fn spawn_process_native<'i, I>(
    source: &cap_std::fs::File,
    args: I,
) -> capnp::Result<tokio::process::Child>
where
    I: IntoIterator<Item = capnp::Result<&'i str>>,
{
    use cap_std::io_lifetimes::raw::AsRawFilelike;
    use std::ffi::OsString;
    use std::process::Stdio;
    use std::str::FromStr;
    use tokio::process::Command;

    let fd = source.as_raw_filelike();

    let argv: Vec<OsString> = args
        .into_iter()
        .map(|x| x.map(|s| OsString::from_str(s).unwrap()))
        .collect::<capnp::Result<Vec<OsString>>>()?;

    let path = format!("/proc/{}/fd/{}", std::process::id(), fd);

    // We can't called fexecve without reimplementing the entire process handling logic, so we just do this
    Command::new(path)
        .args(argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .to_capnp()
}

#[cfg(windows)]
pub fn spawn_process_native<'i, I>(
    source: &cap_std::fs::File,
    args: I,
) -> capnp::Result<tokio::process::Child>
where
    I: IntoIterator<Item = capnp::Result<&'i str>>,
{
    use cap_std::io_lifetimes::raw::AsRawFilelike;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::process::Stdio;
    use std::str::FromStr;
    use tokio::process::Command;
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;

    let argv: Vec<OsString> = args
        .into_iter()
        .map(|x| x.map(|s| OsString::from_str(s).unwrap()))
        .collect::<capnp::Result<Vec<OsString>>>()?;

    let program = unsafe {
        let mut buf = [0_u16; 2048];
        let num = GetFinalPathNameByHandleW(source.as_raw_filelike(), buf.as_mut_ptr(), 2048, 0);
        OsString::from_wide(&buf[0..num as usize])
    };

    // Note: it is actually possible to call libc::wexecve on windows, but this is unreliable.
    Command::new(program)
        .args(argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .to_capnp()
}

use crate::byte_stream_capnp::{
    byte_stream::Client as ByteStreamClient, byte_stream::Owned as ByteStream,
};
use crate::posix_spawn_capnp::{posix_args::Owned as PosixArgs, posix_error::Owned as PosixError};
type PosixProcessClient = process::Client<ByteStream, PosixError>;

use crate::spawn_capnp::program::{SpawnParams, SpawnResults};
use crate::spawn_capnp::{process, program};
use cap_std::fs::File;
use cap_std::io_lifetimes::raw::{FromRawFilelike, RawFilelike};
use capnp_macros::capnproto_rpc;
use eyre::Result;
use std::rc::Rc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::ChildStdin;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
pub type PosixProgramClient = program::Client<PosixArgs, ByteStream, PosixError>;
use atomic_take::AtomicTake;
use capnp_rpc::CapabilityServerSet;
use futures_util::future::BoxFuture;
use futures_util::FutureExt;
use std::cell::RefCell;
use std::io::Write;
use std::process::ExitStatus;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::task::JoinSet;

#[capnproto_rpc(crate::byte_stream_capnp::byte_stream)]
impl crate::byte_stream_capnp::byte_stream::Server for Rc<Mutex<Option<ChildStdin>>> {
    async fn write(&self, bytes: &[u8]) {
        // TODO: This holds our borrow_mut across an await point, which may deadlock
        if let Some(stdin) = self.lock().await.as_mut() {
            match stdin.write_all(bytes).await {
                Ok(_) => stdin.flush().await.to_capnp(),
                Err(e) => Err(capnp::Error::failed(e.to_string())),
            }
        } else {
            Err(capnp::Error::failed(
                "Write called on byte stream after closed.".into(),
            ))
        }
    }

    async fn end(&self) {
        let take = self.lock().await.take();
        if let Some(mut stdin) = take {
            stdin.shutdown().await.to_capnp()
        } else {
            Ok(())
        }
    }

    async fn get_substream(&self) {
        Err(capnp::Error::unimplemented("Not implemented".into()))
    }
}

pub type ProcessCapSet = CapabilityServerSet<PosixProcessImpl, PosixProcessClient>;

#[allow(clippy::type_complexity)]
pub struct PosixProcessImpl {
    pub cancellation_token: CancellationToken,
    pub stdin: Rc<Mutex<Option<ChildStdin>>>,
    pub(crate) future: AtomicTake<BoxFuture<'static, ()>>,
    pub(crate) exitcode: watch::Receiver<Option<Result<ExitStatus>>>,
    pub(crate) killsender: AtomicTake<tokio::sync::oneshot::Sender<()>>,
}

#[allow(clippy::type_complexity)]
impl PosixProcessImpl {
    fn new(
        cancellation_token: CancellationToken,
        stdin: Option<ChildStdin>,
        future: BoxFuture<'static, ()>,
        killsender: tokio::sync::oneshot::Sender<()>,
        exitcode: watch::Receiver<Option<Result<ExitStatus>>>,
    ) -> Self {
        Self {
            cancellation_token,
            stdin: Rc::new(Mutex::new(stdin)),
            future: AtomicTake::new(future),
            killsender: AtomicTake::new(killsender),
            exitcode,
        }
    }

    /// Creates a new instance of `PosixProcessImpl` by spawning a child process.
    ///
    /// **Warning**: This function uses [spawn_local] and must be called in a LocalSet context.
    fn spawn_process<'i, I>(
        program: &cap_std::fs::File,
        args_iter: I,
        stdout_stream: ByteStreamClient,
        _stderr_stream: ByteStreamClient,
    ) -> Result<PosixProcessImpl>
    where
        I: IntoIterator<Item = capnp::Result<&'i str>>,
    {
        // Create the child process
        let mut child = spawn_process_native(program, args_iter)?;

        // Stealing stdin also prevents the `child.wait()` call from closing it.
        let stdin = child.stdin.take();

        // Create a cancellation token to allow us to kill ("cancel") the process.
        let cancellation_token = CancellationToken::new();

        let (killsender, recv) = oneshot::channel::<()>();

        let mut tasks: tokio::task::JoinSet<Result<Option<ExitStatus>>> = JoinSet::new();
        let mut child_stdout = child.stdout.take().unwrap();
        let stdout_token = cancellation_token.child_token();
        // Create the tasks to read stdout and stderr to the byte stream
        tasks.spawn_local(async move {
            tokio::select! {
                _ = stdout_token.cancelled() => Ok(None),
                result = stdout_stream.copy(&mut child_stdout) => result.map(|_| None)
            }
        });

        // TODO: Technically we should be forwarding the child's stderr so it can be redirected,
        // but we don't use that feature right now, so instead we dump it directly to our stderr.
        let stderr_token = cancellation_token.child_token();
        let mut child_stderr = child.stderr.take().unwrap();
        /*tasks.spawn_local(async move {
            tokio::select! {
                _ = stderr_token.cancelled() => Ok(None),
                result = stdout_stream.copy(&mut child_stderr) => result.map(|i| i as i64).map(Some)
            }
        });*/
        let mut stderr = std::io::stderr();
        tasks.spawn_local(async move {
            tokio::select! {
                _ = stderr_token.cancelled() => Ok(None),
                result = async {
                    let mut count = 0;
                    let mut buf = [0u8; 1024];
                    loop {
                        let len = child_stderr.read(&mut buf).await?;

                        if len == 0 {
                            break;
                        }
                        stderr.write_all(&buf[..len])?;
                        count += len;
                      }
                      Ok::<usize, eyre::Report>(count)
                } => result.map(|_| None)
            }
        });

        tasks.spawn_local(async move {
            let r = tokio::select! {
                result = child.wait() => result.map(Some),
                Ok(()) = recv => child.kill().await.map(|_| None),
            }?;

            tracing::debug!("Process exited with status {:?}", r);
            Ok(r)
        });

        let (tx, rx) = watch::channel(None);
        let future = async move {
            while let Some(result) = tasks.join_next().await {
                match result {
                    Ok(Ok(Some(e))) => {
                        tx.send_replace(Some(Ok(e)));
                    }
                    Ok(Ok(None)) => (),
                    Ok(Err(e)) => {
                        tx.send_replace(Some(Err(e)));
                    }
                    Err(e) => {
                        tx.send_replace(Some(Err(eyre::eyre!(e.to_string()))));
                    }
                }
            }
            tasks.shutdown().await;
        };

        Result::Ok(Self::new(
            cancellation_token,
            stdin,
            future.boxed(),
            killsender,
            rx,
        ))
    }
}

type GetApiParams = process::GetApiParams<ByteStream, PosixError>;
type GetApiResults = process::GetApiResults<ByteStream, PosixError>;
type GetErrorParams = process::GetErrorParams<ByteStream, PosixError>;
type GetErrorResults = process::GetErrorResults<ByteStream, PosixError>;
type KillParams = process::KillParams<ByteStream, PosixError>;
type KillResults = process::KillResults<ByteStream, PosixError>;
type JoinParams = process::JoinParams<ByteStream, PosixError>;
type JoinResults = process::JoinResults<ByteStream, PosixError>;

impl PosixProcessImpl {
    async fn finish(&self) -> capnp::Result<i32> {
        let mut exitcode = self.exitcode.clone();
        let status = exitcode.wait_for(|x| x.is_some()).await.to_capnp()?;

        if let Some(v) = status.as_ref() {
            // TODO: use std::os::unix::process::ExitStatusExt on unix to handle None
            Ok(v.as_ref().to_capnp()?.code().unwrap_or(i32::MIN))
        } else {
            Ok(i32::MIN)
        }
    }
}
impl process::Server<ByteStream, PosixError> for PosixProcessImpl {
    /// In this implementation of `spawn`, the functions returns the exit code of the child
    /// process.
    async fn get_error(
        &self,
        _: GetErrorParams,
        mut results: GetErrorResults,
    ) -> capnp::Result<()> {
        let results_builder = results.get();
        let mut process_error_builder = results_builder.init_result();

        if let Some(code) = self.exitcode.borrow().as_ref() {
            process_error_builder.set_error_code(
                code.as_ref()
                    .map(|v| v.code().unwrap_or(i32::MIN) as i64)
                    .unwrap_or(i64::MIN),
            );
        }

        Ok(())
    }

    async fn kill(&self, _: KillParams, _: KillResults) -> capnp::Result<()> {
        if let Some(sender) = self.killsender.take() {
            let _ = sender.send(());
            self.cancellation_token.cancel();
            self.finish().await.map(|_| ())
        } else {
            Ok(())
        }
    }

    async fn get_api(
        &self,
        _params: GetApiParams,
        mut results: GetApiResults,
    ) -> capnp::Result<()> {
        results
            .get()
            .set_api(capnp_rpc::new_client(self.stdin.clone()))
    }

    async fn join(&self, _: JoinParams, mut results: JoinResults) -> capnp::Result<()> {
        let results_builder = results.get();
        let mut process_error_builder = results_builder.init_result();
        process_error_builder.set_error_code(self.finish().await?.into());
        Ok(())
    }
}

pub struct PosixProgramImpl {
    program: File,
    process_set: Rc<RefCell<crate::posix_process::ProcessCapSet>>,
}

impl PosixProgramImpl {
    pub fn new(handle: u64, process_set: Rc<RefCell<crate::posix_process::ProcessCapSet>>) -> Self {
        unsafe {
            Self {
                program: File::from_raw_filelike(handle as RawFilelike),
                process_set,
            }
        }
    }
    pub fn new_std(
        file: std::fs::File,
        process_set: Rc<RefCell<crate::posix_process::ProcessCapSet>>,
    ) -> Self {
        Self {
            program: cap_std::fs::File::from_std(file),
            process_set,
        }
    }
}

impl program::Server<PosixArgs, ByteStream, PosixError> for PosixProgramImpl {
    async fn spawn(
        &self,
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

        match PosixProcessImpl::spawn_process(&self.program, argv_iter, stdout, stderr) {
            Err(e) => Err(capnp::Error::failed(e.to_string())),
            Ok(process_impl) => {
                let process_client: PosixProcessClient =
                    self.process_set.borrow_mut().new_client(process_impl);
                results.get().set_result(process_client);
                Ok(())
            }
        }
    }
}
/*
#[cfg(test)]
mod tests {
    use super::Rc;
    use super::RefCell;
    use super::{PosixProgramClient, PosixProgramImpl};
    use crate::byte_stream::ByteStreamImpl;
    use cap_std::io_lifetimes::{FromFilelike, IntoFilelike};
    use std::fs::File;
    use tokio::task;

    #[tokio::test]
    async fn test_posix_process_creation() {
        let spawn_process_server = PosixProgramImpl {
            #[cfg(windows)]
            program: cap_std::fs::File::from_filelike(
                File::open("C:/Windows/System32/cmd.exe")
                    .unwrap()
                    .into_filelike(),
            ),
            #[cfg(not(windows))]
            program: cap_std::fs::File::from_filelike(
                File::open("/bin/sh").unwrap().into_filelike(),
            ),
            process_set: Rc::new(RefCell::new(crate::posix_process::ProcessCapSet::new())),
        };
        let spawn_process_client: PosixProgramClient = capnp_rpc::new_client(spawn_process_server);

        let e = task::LocalSet::new()
            .run_until(async {
                // Setting up stuff needed for RPC
                let stdout_server = ByteStreamImpl::new(|bytes| {
                    println!("remote stdout: {}", std::str::from_utf8(bytes).unwrap());
                    std::future::ready(Ok(()))
                });
                let stdout_client: super::ByteStreamClient = capnp_rpc::new_client(stdout_server);
                let stderr_server = ByteStreamImpl::new(|bytes| {
                    println!("remote stderr: {}", std::str::from_utf8(bytes).unwrap());
                    std::future::ready(Ok(()))
                });
                let stderr_client: super::ByteStreamClient = capnp_rpc::new_client(stderr_server);

                let mut spawn_request = spawn_process_client.spawn_request();
                let params_builder = spawn_request.get();
                let mut args_builder = params_builder.init_args();
                args_builder.set_stdout(stdout_client);
                args_builder.set_stderr(stderr_client);
                let mut list_builder = args_builder.init_args(2);

                #[cfg(windows)]
                list_builder.set(0, "/C".into());
                #[cfg(not(windows))]
                list_builder.set(0, "-c".into());

                #[cfg(windows)]
                list_builder.set(1, r#"echo "Hello World!" & exit 2"#.into());
                #[cfg(not(windows))]
                list_builder.set(1, r#"echo "Hello World!"; exit 2"#.into());

                let response = spawn_request.send().promise.await?;
                let process_client = response.get()?.get_result()?;

                let join_response = process_client.join_request().send().promise.await?;
                let error_reader = join_response.get()?.get_result()?;

                let error_code = error_reader.get_error_code();
                assert_eq!(error_code, 2);

                let error_message = error_reader.get_error_message()?;
                assert!(error_message.is_empty() == true);

                Ok::<(), eyre::Error>(())
            })
            .await;

        e.unwrap();
    }
}
*/
