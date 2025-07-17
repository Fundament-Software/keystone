#[cfg(not(windows))]
pub fn spawn_process_native<'i, I>(
    source: &cap_std::fs::File,
    args: I,
    log_filter: &str,
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

    let mut argv: Vec<OsString> = args
        .into_iter()
        .map(|x| x.map(|s| OsString::from_str(s).unwrap()))
        .collect::<capnp::Result<Vec<OsString>>>()?;

    let path = format!("/proc/{}/fd/{}", std::process::id(), fd);

    // We can't called fexecve without reimplementing the entire process handling logic, so we just do this
    let mut cmd = Command::new(path);
    cmd.args(argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());

    if !log_filter.is_empty() {
        cmd.env("KEYSTONE_MODULE_LOG", log_filter);
    }

    cmd.spawn().to_capnp()
}

#[cfg(not(windows))]
fn create_ipc() -> Result<(UnixListener, String, tempfile::TempDir)> {
    let random = rand::random::<u8>() as char; //TODO security and better name
    let dir = tempfile::tempdir()?;
    let pipe_name = dir
        .path()
        .join(random.to_string().as_str())
        .to_str()
        .unwrap()
        .to_owned();
    let server = UnixListener::bind(pipe_name.clone())?;
    Ok((server, pipe_name, dir))
}

#[cfg(windows)]
pub fn spawn_process_native<'i, I>(
    source: &cap_std::fs::File,
    args: I,
    log_filter: &str,
    pipe_name: std::ffi::OsString,
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

    let mut argv: Vec<OsString> = args
        .into_iter()
        .map(|x| x.map(|s| OsString::from_str(s).unwrap()))
        .collect::<capnp::Result<Vec<OsString>>>()?;
    argv.insert(0, pipe_name);

    let program = unsafe {
        let mut buf = [0_u16; 2048];
        let num = GetFinalPathNameByHandleW(source.as_raw_filelike(), buf.as_mut_ptr(), 2048, 0);
        OsString::from_wide(&buf[0..num as usize])
    };

    // Note: it is actually possible to call libc::wexecve on windows, but this is unreliable.
    let mut cmd = Command::new(program);
    cmd.args(argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());

    if !log_filter.is_empty() {
        cmd.env("KEYSTONE_MODULE_LOG", log_filter);
    }

    cmd.spawn().to_capnp()
}

#[cfg(windows)]
fn create_ipc() -> Result<(NamedPipeServer, String)> {
    let random = rand::random::<u8>(); //TODO security and better name
    let mut pipe_name = r"\\.\pipe\".to_string();
    pipe_name.push(random as char);
    let server = ServerOptions::new().create(&pipe_name)?;
    Ok((server, pipe_name))
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
use tokio::io::{AsyncWriteExt, ReadHalf, WriteHalf};
#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
#[cfg(not(windows))]
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
pub type PosixProgramClient = program::Client<PosixArgs, ByteStream, PosixError>;
use crate::capnp_rpc::{self, CapabilityServerSet};
use crate::keystone::CapnpResult;
use atomic_take::AtomicTake;
use futures_util::FutureExt;
use futures_util::future::BoxFuture;
use futures_util::future::LocalBoxFuture;
use std::cell::RefCell;
use std::io::Write;
use std::process::ExitStatus;
use tokio::io::AsyncReadExt;
use tokio::sync::oneshot;
use tokio::sync::watch;

#[cfg(windows)]
#[capnproto_rpc(crate::byte_stream_capnp::byte_stream)]
impl crate::byte_stream_capnp::byte_stream::Server for Mutex<Option<WriteHalf<NamedPipeServer>>> {
    async fn write(self: Rc<Self>, bytes: &[u8]) {
        // TODO: This holds our borrow_mut across an await point, which may deadlock
        if let Some(stdin) = self.lock().await.as_mut() {
            match stdin.write_all(bytes).await {
                Ok(_) => stdin.flush().await.to_capnp(),
                Err(e) => Err(capnp::Error::failed(e.to_string())),
            }
        } else {
            // Because capnproto is stupid, we cannot error here, since it will always try to write
            // to a pipe that has already been closed even during a graceful exit.
            Ok(())
        }
    }

    async fn end(self: Rc<Self>) {
        let take = self.lock().await.take();
        if let Some(mut stdin) = take {
            tracing::debug!("Closing stdin pipe");
            stdin.shutdown().await.to_capnp()
        } else {
            Ok(())
        }
    }

    async fn get_substream(self: Rc<Self>) {
        Err(capnp::Error::unimplemented("Not implemented".into()))
    }
}

#[cfg(not(windows))]
#[capnproto_rpc(crate::byte_stream_capnp::byte_stream)]
impl crate::byte_stream_capnp::byte_stream::Server
    for Mutex<Option<tokio::net::unix::OwnedWriteHalf>>
{
    async fn write(self: Rc<Self>, bytes: &[u8]) {
        // TODO: This holds our borrow_mut across an await point, which may deadlock
        if let Some(stdin) = self.lock().await.as_mut() {
            match stdin.write_all(bytes).await {
                Ok(_) => stdin.flush().await.to_capnp(),
                Err(e) => Err(capnp::Error::failed(e.to_string())),
            }
        } else {
            // Because capnproto is stupid, we cannot error here, since it will always try to write
            // to a pipe that has already been closed even during a graceful exit.
            Ok(())
        }
    }

    async fn end(self: Rc<Self>) {
        let take = self.lock().await.take();
        if let Some(mut stdin) = take {
            stdin.shutdown().await.to_capnp()
        } else {
            Ok(())
        }
    }

    async fn get_substream(self: Rc<Self>) {
        Err(capnp::Error::unimplemented("Not implemented".into()))
    }
}

pub type ProcessCapSet = CapabilityServerSet<PosixProcessImpl, PosixProcessClient>;

#[allow(clippy::type_complexity)]
pub struct PosixProcessImpl {
    pub cancellation_token: CancellationToken,
    #[cfg(windows)]
    pub stdin: Rc<Mutex<Option<WriteHalf<NamedPipeServer>>>>,
    #[cfg(not(windows))]
    pub stdin: Rc<Mutex<Option<tokio::net::unix::OwnedWriteHalf>>>,
    pub(crate) process: AtomicTake<BoxFuture<'static, Result<ExitStatus>>>,
    pub(crate) stdout: AtomicTake<LocalBoxFuture<'static, Result<ReadHalf<NamedPipeServer>>>>,
    pub(crate) stderr: AtomicTake<LocalBoxFuture<'static, Result<usize>>>,
    pub(crate) exitcode: watch::Receiver<Option<ExitStatus>>,
    pub(crate) killsender: AtomicTake<tokio::sync::oneshot::Sender<()>>,
}

#[allow(clippy::type_complexity)]
impl PosixProcessImpl {
    fn new(
        cancellation_token: CancellationToken,
        #[cfg(windows)] stdin: Option<WriteHalf<NamedPipeServer>>,
        #[cfg(not(windows))] stdin: Option<tokio::net::unix::OwnedWriteHalf>,
        process: BoxFuture<'static, Result<ExitStatus>>,
        stdout: LocalBoxFuture<'static, Result<ReadHalf<NamedPipeServer>>>,
        stderr: LocalBoxFuture<'static, Result<usize>>,
        killsender: tokio::sync::oneshot::Sender<()>,
        exitcode: watch::Receiver<Option<ExitStatus>>,
    ) -> Self {
        Self {
            cancellation_token,
            stdin: Rc::new(Mutex::new(stdin)),
            process: AtomicTake::new(process),
            stdout: AtomicTake::new(stdout),
            stderr: AtomicTake::new(stderr),
            killsender: AtomicTake::new(killsender),
            exitcode,
        }
    }

    /// Creates a new instance of `PosixProcessImpl` by spawning a child process.
    async fn spawn_process<'i, I>(
        program: &cap_std::fs::File,
        args_iter: I,
        log_filter: &str,
        stdout_stream: ByteStreamClient,
        stderr_stream: ByteStreamClient,
    ) -> Result<PosixProcessImpl>
    where
        I: IntoIterator<Item = capnp::Result<&'i str>>,
    {
        #[cfg(not(windows))]
        let (server, backup_reserve_fd) = unsafe {
            //TODO Thread unsafe, might need a lock in the future
            use std::os::fd::FromRawFd;

            let backup_reserve_fd = libc::fcntl(4, libc::F_DUPFD_CLOEXEC, 5);
            if backup_reserve_fd < 0 {
                panic!("fcntl(4, F_DUPFD_CLOEXEC, 5) failed");
            }
            let mut socks = vec![0 as i32, 0 as i32];
            if libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                0,
                socks.as_mut_ptr(),
            ) < 0
            {
                libc::close(backup_reserve_fd);
                panic!("socketpair failed");
            }
            if libc::dup2(socks[1], 4) != 4 {
                libc::close(backup_reserve_fd);
                panic!("dup2(socks[1], 4) failed");
            }
            libc::close(socks[1]);
            let server =
                UnixStream::from_std(std::os::unix::net::UnixStream::from_raw_fd(socks[0]))
                    .unwrap();
            (server, backup_reserve_fd)
        };
        #[cfg(windows)]
        let (server, pipe_name) = create_ipc()?;

        // Create the child process
        #[cfg(windows)]
        let mut child = spawn_process_native(program, args_iter, log_filter, pipe_name.into())?;
        #[cfg(not(windows))]
        let mut child = spawn_process_native(program, args_iter, log_filter)?;

        // Stealing stdin also prevents the `child.wait()` call from closing it.
        //let test = child.stdin.take();

        // Create a cancellation token to allow us to kill ("cancel") the process.
        let cancellation_token = CancellationToken::new();

        let (killsender, recv) = oneshot::channel::<()>();

        #[cfg(windows)]
        server.connect().await?;
        tracing::debug!("connecting to pipe");

        #[cfg(windows)]
        let (mut read, write) = tokio::io::split(server);
        #[cfg(not(windows))]
        let (mut read, write) = server.into_split();
        #[cfg(not(windows))]
        unsafe {
            libc::dup2(backup_reserve_fd, 4);
            libc::close(backup_reserve_fd);
        }

        //let mut child_stdout = child.stdout.take().unwrap();
        let pipe_token = cancellation_token.child_token();
        // Create the tasks to read stdout and stderr to the byte stream
        let pipe_future = async move {
            tokio::select! {
                _ = pipe_token.cancelled() => Ok(0),
                result = stdout_stream.copy(&mut read) => result
            }?;
            let _ = stdout_stream.end_request().send().promise.await;
            Ok(read)
        };

        let stderr_token = cancellation_token.child_token();
        let mut child_stderr = child.stderr.take().unwrap();
        /*tasks.spawn_local(async move {
            let r = tokio::select! {
                _ = stderr_token.cancelled() => Ok(0),
                result = stderr_stream.copy(&mut child_stderr) => result
            };
            let _ = stderr_stream.end_request().send().promise.await;
            r
        });*/

        let mut stderr = std::io::stderr();
        let stderr_future = async move {
            let r = tokio::select! {
                _ = stderr_token.cancelled() => Ok(0),
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
                } => result
            }?;
            let _ = stderr_stream.end_request().send().promise.await;
            Ok(r)
        };

        let (tx, rx) = watch::channel(None);
        let process_future = async move {
            let r = tokio::select! {
                result = child.wait() => { tracing::debug!("child {:?} returned {:?}", child.id(), &result); result.map(Some) },
                Ok(()) = recv => { tracing::warn!("kill signal sent to {:?}", child.id()); child.kill().await.map(|_| None) },
            }?;

            tracing::debug!("Process exited with status {:?}", r);
            tx.send_replace(r.clone());
            Ok(r.unwrap_or_default())
        };
        Result::Ok(Self::new(
            cancellation_token,
            Some(write),
            process_future.boxed(),
            pipe_future.boxed_local(),
            stderr_future.boxed_local(),
            killsender,
            rx,
        ))
    }
}

impl PosixProcessImpl {
    #[allow(clippy::needless_return)]
    async fn finish(&self) -> capnp::Result<i32> {
        tracing::debug!("finish() called");
        #[cfg(not(windows))]
        use std::os::unix::process::ExitStatusExt;

        // TODO: Taking away stdin here ensures that no errant writes will ever make it through a closed pipe,
        // because this is called directly after a stop_request has returned. However, this also means we are closing
        // this side of the pipe before the module has finished emptying it's queue, which may have unintended
        // consequences.
        let _ = self.stdin.lock().await.take();
        let mut exitcode = self.exitcode.clone();
        let status = exitcode.wait_for(|x| x.is_some()).await.to_capnp()?;

        if let Some(v) = status.as_ref() {
            #[cfg(not(windows))]
            return Ok(v.code().unwrap_or_else(|| v.signal().unwrap_or(i32::MIN)));

            #[cfg(windows)]
            return Ok(v.code().unwrap_or(i32::MIN));
        } else {
            return Ok(i32::MIN);
        }
    }
}

#[capnproto_rpc(process)]
impl process::Server<ByteStream, PosixError> for PosixProcessImpl {
    /// In this implementation of `spawn`, the functions returns the exit code of the child process
    async fn get_error(self: Rc<Self>) -> capnp::Result<()> {
        let results_builder = results.get();
        let mut process_error_builder = results_builder.init_result();

        if let Some(code) = self.exitcode.borrow().as_ref() {
            process_error_builder.set_error_code(code.code().unwrap_or(i32::MIN) as i64);
        }

        Ok(())
    }

    async fn kill(self: Rc<Self>) -> capnp::Result<()> {
        tracing::debug!("kill() called");

        if let Some(sender) = self.killsender.take() {
            let _ = sender.send(());
            self.cancellation_token.cancel();
            self.finish().await.map(|_| ())
        } else {
            Ok(())
        }
    }

    async fn get_api(self: Rc<Self>) -> capnp::Result<()> {
        results
            .get()
            .set_api(capnp_rpc::new_client_from_rc(self.stdin.clone()))
    }

    async fn join(self: Rc<Self>) -> capnp::Result<()> {
        let results_builder = results.get();
        let mut process_error_builder = results_builder.init_result();
        process_error_builder.set_error_code(self.finish().await?.into());
        Ok(())
    }
}

pub struct PosixProgramImpl {
    program: File,
    process_set: Rc<RefCell<crate::posix_process::ProcessCapSet>>,
    log_filter: String,
}

impl PosixProgramImpl {
    pub fn new(
        handle: u64,
        process_set: Rc<RefCell<crate::posix_process::ProcessCapSet>>,
        log_filter: String,
    ) -> Self {
        unsafe {
            Self {
                program: File::from_raw_filelike(handle as RawFilelike),
                process_set,
                log_filter,
            }
        }
    }
    pub fn new_std(
        file: std::fs::File,
        process_set: Rc<RefCell<crate::posix_process::ProcessCapSet>>,
        log_filter: String,
    ) -> Self {
        Self {
            program: cap_std::fs::File::from_std(file),
            process_set,
            log_filter,
        }
    }
}

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

        match PosixProcessImpl::spawn_process(
            &self.program,
            argv_iter,
            &self.log_filter,
            stdout,
            stderr,
        )
        .await
        {
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
