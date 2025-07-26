use crate::byte_stream_capnp::{
    byte_stream::Client as ByteStreamClient, byte_stream::Owned as ByteStream,
};
use crate::posix_capnp::{posix_args::Owned as PosixArgs, posix_error::Owned as PosixError};
type PosixProcessClient = process::Client<ByteStream, PosixError>;

use crate::spawn_capnp::{process, program};
use capnp_macros::capnproto_rpc;
use eyre::Result;
use std::rc::Rc;
use tokio::io::AsyncWriteExt;
#[cfg(windows)]
use tokio::io::{ReadHalf, WriteHalf};
#[cfg(windows)]
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
pub type PosixProgramClient = program::Client<PosixArgs, ByteStream, PosixError>;
use crate::capnp;
use crate::capnp_rpc::{self, CapabilityServerSet};
use crate::keystone::CapnpResult;
use atomic_take::AtomicTake;
use futures_util::future::BoxFuture;
use futures_util::future::LocalBoxFuture;
use std::process::ExitStatus;
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
    #[cfg(windows)]
    pub(crate) stdout: AtomicTake<LocalBoxFuture<'static, Result<ReadHalf<NamedPipeServer>>>>,
    #[cfg(not(windows))]
    pub(crate) stdout: AtomicTake<LocalBoxFuture<'static, Result<tokio::net::unix::OwnedReadHalf>>>,
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
        #[cfg(windows)] stdout: LocalBoxFuture<'static, Result<ReadHalf<NamedPipeServer>>>,
        #[cfg(not(windows))] stdout: LocalBoxFuture<
            'static,
            Result<tokio::net::unix::OwnedReadHalf>,
        >,
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
    pub async fn spawn_process<'i, I>(
        program: &cap_std::fs::File,
        args_iter: I,
        log_filter: &str,
        stdout_stream: ByteStreamClient,
        stderr_stream: ByteStreamClient,
    ) -> Result<PosixProcessImpl>
    where
        I: IntoIterator<Item = capnp::Result<&'i str>>,
    {
        todo!()
        // Stealing stdin also prevents the `child.wait()` call from closing it.
        //let test = child.stdin.take();
        /*
        // Create a cancellation token to allow us to kill ("cancel") the process.
        let cancellation_token = CancellationToken::new();

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
            tx.send_replace(r);
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
        ))*/
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
