use crate::keystone::CapnpResult;

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

    let argv: Vec<OsString> = args
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
fn create_ipc() -> (UnixStream, String) {
    let random = rand::random::<u8>(); //TODO security and better name
    let mut pipe_name = r"/dev/null/".to_string();
    pipe_name.push(random as char);
    let server = UnixStream::connect(&pipe_name).unwrap();
    (server, pipe_name)
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
    use tokio::net::windows::named_pipe::ClientOptions;
    use tokio::process::Command;
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;

    let mut argv: Vec<OsString> = args
        .into_iter()
        .map(|x| x.map(|s| OsString::from_str(s).unwrap()))
        .collect::<capnp::Result<Vec<OsString>>>()?;

    argv.push(pipe_name);

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
fn create_ipc() -> (NamedPipeServer, String) {
    let random = rand::random::<u8>(); //TODO security and better name
    let mut pipe_name = r"\\.\pipe\".to_string();
    pipe_name.push(random as char);
    let server = ServerOptions::new().create(&pipe_name).unwrap();
    (server, pipe_name)
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
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::process::ChildStdin;
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
pub type PosixProgramClient = program::Client<PosixArgs, ByteStream, PosixError>;
use atomic_take::AtomicTake;
use capnp_rpc::CapabilityServerSet;
use futures_util::FutureExt;
use futures_util::future::BoxFuture;
use std::cell::RefCell;
use std::process::ExitStatus;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::task::JoinSet;

#[capnproto_rpc(crate::byte_stream_capnp::byte_stream)]
impl crate::byte_stream_capnp::byte_stream::Server for Mutex<Option<ChildStdin>> {
    async fn write(self: Rc<Self>, bytes: &[u8]) {
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
            Err(capnp::Error::failed(
                "Write called on byte stream after closed.".into(),
            ))
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

#[cfg(windows)]
#[allow(clippy::type_complexity)]
pub struct PosixProcessImpl {
    pub cancellation_token: CancellationToken,
    pub stdin: Rc<Mutex<Option<WriteHalf<NamedPipeServer>>>>,
    pub(crate) future: AtomicTake<BoxFuture<'static, Result<ExitStatus>>>,
    pub(crate) exitcode: watch::Receiver<Option<Result<ExitStatus>>>,
    pub(crate) killsender: AtomicTake<tokio::sync::oneshot::Sender<()>>,
    test: Option<ChildStdin>,
}

#[cfg(not(windows))]
#[allow(clippy::type_complexity)]
pub struct PosixProcessImpl {
    pub cancellation_token: CancellationToken,
    pub stdin: Rc<Mutex<Option<WriteHalf<NamedPipeServer>>>>,
    pub(crate) future: AtomicTake<BoxFuture<'static, Result<ExitStatus>>>,
    pub(crate) exitcode: watch::Receiver<Option<Result<ExitStatus>>>,
    pub(crate) killsender: AtomicTake<tokio::sync::oneshot::Sender<()>>,
}

#[allow(clippy::type_complexity)]
impl PosixProcessImpl {
    fn new(
        cancellation_token: CancellationToken,
        stdin: Option<WriteHalf<NamedPipeServer>>,
        future: BoxFuture<'static, Result<ExitStatus>>,
        killsender: tokio::sync::oneshot::Sender<()>,
        exitcode: watch::Receiver<Option<Result<ExitStatus>>>,
        test: Option<ChildStdin>,
    ) -> Self {
        Self {
            cancellation_token,
            stdin: Rc::new(Mutex::new(stdin)),
            future: AtomicTake::new(future),
            killsender: AtomicTake::new(killsender),
            exitcode,
            test,
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
        let (server, pipe_name) = create_ipc();
        // Create the child process
        let mut child = spawn_process_native(program, args_iter, log_filter, pipe_name.into())?;

        // Stealing stdin also prevents the `child.wait()` call from closing it.
        let test = child.stdin.take();

        // Create a cancellation token to allow us to kill ("cancel") the process.
        let cancellation_token = CancellationToken::new();

        let (killsender, recv) = oneshot::channel::<()>();

        let mut tasks: tokio::task::JoinSet<Result<Option<ExitStatus>>> = JoinSet::new();
        server.connect().await.unwrap();
        let (mut read, mut write) = tokio::io::split(server);
        let stdin = Some(write);
        //let mut child_stdout = child.stdout.take().unwrap();
        let stdout_token = cancellation_token.child_token();
        // Create the tasks to read stdout and stderr to the byte stream
        tasks.spawn_local(async move {
            let r = tokio::select! {
                _ = stdout_token.cancelled() => Ok(None),
                result = stdout_stream.copy(&mut read) => result.map(|_| None)
            };
            let _ = stdout_stream.end_request().send().promise.await;
            r
        });

        let stderr_token = cancellation_token.child_token();
        let mut child_stderr = child.stderr.take().unwrap();
        tasks.spawn_local(async move {
            let r = tokio::select! {
                _ = stderr_token.cancelled() => Ok(Some(ExitStatus::default())),
                result = stderr_stream.copy(&mut child_stderr) => result.map(|_| None)
            };
            let _ = stderr_stream.end_request().send().promise.await;
            r
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
            let mut r = Err(eyre::eyre!("No tasks spawned"));
            while let Some(result) = tasks.join_next().await {
                match result {
                    Ok(Ok(Some(e))) => {
                        tx.send_replace(Some(Ok(e)));
                        r = Ok(e);
                    }
                    Ok(Ok(None)) => (),
                    Ok(Err(e)) => {
                        r = Err(eyre::eyre!(e.to_string())); // We call to_string here because we have to copy the error
                        tx.send_replace(Some(Err(e)));
                    }
                    Err(e) => {
                        r = Err(eyre::eyre!(e.to_string()));
                        tx.send_replace(Some(Err(e.into())));
                    }
                }
            }
            tasks.shutdown().await;
            r
        };

        Result::Ok(Self::new(
            cancellation_token,
            stdin,
            future.boxed(),
            killsender,
            rx,
            test,
        ))
    }
}

impl PosixProcessImpl {
    #[allow(clippy::needless_return)]
    async fn finish(&self) -> capnp::Result<i32> {
        #[cfg(not(windows))]
        use std::os::unix::process::ExitStatusExt;

        let mut exitcode = self.exitcode.clone();
        let status = exitcode.wait_for(|x| x.is_some()).await.to_capnp()?;

        if let Some(v) = status.as_ref() {
            let status = v.as_ref().to_capnp()?;
            #[cfg(not(windows))]
            return Ok(status
                .code()
                .unwrap_or_else(|| status.signal().unwrap_or(i32::MIN)));

            #[cfg(windows)]
            return Ok(status.code().unwrap_or(i32::MIN));
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
            process_error_builder.set_error_code(
                code.as_ref()
                    .map(|v| v.code().unwrap_or(i32::MIN) as i64)
                    .unwrap_or(i64::MIN),
            );
        }

        Ok(())
    }

    async fn kill(self: Rc<Self>) -> capnp::Result<()> {
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
