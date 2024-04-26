#[cfg(not(windows))]
pub fn spawn_process_native<'i, I>(
    source: &cap_std::fs::File,
    args: I,
) -> Result<tokio::process::Child, capnp::Error>
where
    I: IntoIterator<Item = Result<&'i str, capnp::Error>>,
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
        .collect::<Result<Vec<OsString>, capnp::Error>>()?;

    let path = format!("/proc/{}/fd/{}", std::process::id(), fd);

    // We can't called fexecve without reimplementing the entire process handling logic, so we just do this
    Command::new(path)
        .args(argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| capnp::Error::failed(e.to_string()))
}

#[cfg(windows)]
pub fn spawn_process_native<'i, I>(
    source: &cap_std::fs::File,
    args: I,
) -> Result<tokio::process::Child, capnp::Error>
where
    I: IntoIterator<Item = Result<&'i str, capnp::Error>>,
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
        .collect::<Result<Vec<OsString>, capnp::Error>>()?;

    let program = unsafe {
        let mut buf = [0_u16; 2048];
        let num =
            GetFinalPathNameByHandleW(source.as_raw_filelike() as isize, buf.as_mut_ptr(), 2048, 0);
        OsString::from_wide(&buf[0..num as usize])
    };

    // Note: it is actually possible to call libc::wexecve on windows, but this is unreliable.
    Command::new(program)
        .args(argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| capnp::Error::failed(e.to_string()))
}

pub mod posix_process {
    use crate::byte_stream_capnp::{
        byte_stream::Client as ByteStreamClient, byte_stream::Owned as ByteStream,
    };
    use crate::posix_spawn_capnp::{
        posix_args::Owned as PosixArgs, posix_error::Owned as PosixError,
    };
    use crate::spawn_capnp::program::{SpawnParams, SpawnResults};
    use crate::spawn_capnp::{process, program};
    use cap_std::fs::File;
    use cap_std::io_lifetimes::raw::{FromRawFilelike, RawFilelike};
    use capnp_macros::capnproto_rpc;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::result::Result;
    use tokio::io::{AsyncRead, AsyncWriteExt};
    use tokio::process::Child;
    use tokio::process::ChildStdin;
    use tokio::task::{spawn_local, JoinHandle};
    use tokio_util::sync::CancellationToken;

    pub type PosixProgramClient = program::Client<PosixArgs, ByteStream, PosixError>;

    pub struct PosixProcessImpl {
        pub cancellation_token: CancellationToken,
        stdin: Rc<RefCell<Option<ChildStdin>>>,
        stdout_task: JoinHandle<eyre::Result<Option<usize>>>,
        stderr_task: JoinHandle<eyre::Result<Option<usize>>>,
        child: Child,
    }

    #[capnproto_rpc(crate::byte_stream_capnp::byte_stream)]
    impl crate::byte_stream_capnp::byte_stream::Server for Rc<RefCell<Option<ChildStdin>>> {
        #[async_backtrace::framed]
        async fn write(&self, bytes: &[u8]) {
            if let Some(stdin) = self.borrow_mut().as_mut() {
                match stdin.write_all(bytes).await {
                    Ok(_) => stdin
                        .flush()
                        .await
                        .map_err(|e| capnp::Error::failed(e.to_string())),
                    Err(e) => Err(capnp::Error::failed(e.to_string())),
                }
            } else {
                Err(capnp::Error {
                    kind: capnp::ErrorKind::Failed,
                    extra: String::from("Write called on byte stream after closed."),
                })
            }
        }

        #[async_backtrace::framed]
        async fn end(&self) {
            if let Some(mut stdin) = self.borrow_mut().take() {
                stdin
                    .shutdown()
                    .await
                    .map_err(|e| capnp::Error::failed(e.to_string()))
            } else {
                Ok(())
            }
        }

        #[async_backtrace::framed]
        async fn get_substream(&self) {
            Err(capnp::Error {
                kind: capnp::ErrorKind::Unimplemented,
                extra: String::from("Not implemented"),
            })
        }
    }

    /// Helper function for PosixProcessImpl::spawn_process.
    /// Creates a task on the localset that reads the `AsyncRead` (usually a
    /// ChildStdout/ChildStderr) and copies to the ByteStreamClient.
    ///
    /// **Warning**: This function uses [spawn_local] and must be called in a LocalSet context.
    fn spawn_iostream_task(
        mut stream: impl AsyncRead + Unpin + 'static, // In this case, 'static means an owned type. Also required for spawn_local
        bytestream: ByteStreamClient,
        cancellation_token: CancellationToken,
    ) -> JoinHandle<eyre::Result<Option<usize>>> {
        spawn_local(async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => Ok(None),
                result = bytestream.copy(&mut stream) => result.map(Some)
            }
        })
    }

    impl PosixProcessImpl {
        fn new(
            cancellation_token: CancellationToken,
            stdin: Option<ChildStdin>,
            stdout_task: JoinHandle<eyre::Result<Option<usize>>>,
            stderr_task: JoinHandle<eyre::Result<Option<usize>>>,
            child: Child,
        ) -> Self {
            Self {
                cancellation_token,
                stdin: Rc::new(RefCell::new(stdin)),
                stdout_task,
                stderr_task,
                child,
            }
        }

        /// Creates a new instance of `PosixProcessImpl` by spawning a child process.
        ///
        /// **Warning**: This function uses [spawn_local] and must be called in a LocalSet context.
        fn spawn_process<'i, I>(
            program: &cap_std::fs::File,
            args_iter: I,
            stdout_stream: ByteStreamClient,
            stderr_stream: ByteStreamClient,
        ) -> eyre::Result<PosixProcessImpl>
        where
            I: IntoIterator<Item = Result<&'i str, capnp::Error>>,
        {
            // Create the child process
            let mut child = super::spawn_process_native(program, args_iter)?;

            // Stealing stdin also prevents the `child.wait()` call from closing it.
            let stdin = child.stdin.take();

            // Create a cancellation token to allow us to kill ("cancel") the process.
            let cancellation_token = CancellationToken::new();

            // Create the tasks to read stdout and stderr to the byte stream
            let stdout_task = spawn_iostream_task(
                child.stdout.take().unwrap(),
                stdout_stream,
                cancellation_token.child_token(),
            );

            let stderr_task = spawn_iostream_task(
                child.stderr.take().unwrap(),
                stderr_stream,
                cancellation_token.child_token(),
            );

            eyre::Result::Ok(Self::new(
                cancellation_token,
                stdin,
                stdout_task,
                stderr_task,
                child,
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
    type PosixProcessClient = process::Client<ByteStream, PosixError>;

    // You might ask why this trait is implementated for an `Rc` + `RefCell` of `PosixProcessImpl`
    // Well thats a very good question
    impl process::Server<ByteStream, PosixError> for Rc<RefCell<PosixProcessImpl>> {
        /// In this implementation of `spawn`, the functions returns the exit code of the child
        /// process.
        #[async_backtrace::framed]
        async fn get_error(
            &self,
            _: GetErrorParams,
            mut results: GetErrorResults,
        ) -> Result<(), capnp::Error> {
            let results_builder = results.get();
            let mut process_error_builder = results_builder.init_result();

            match self.borrow_mut().child.try_wait() {
                Ok(Some(exitstatus)) => {
                    // TODO: use std::os::unix::process::ExitStatusExt on unix to handle None
                    let exitcode = exitstatus.code().unwrap_or(i32::MIN);
                    process_error_builder.set_error_code(exitcode.into());

                    Ok(())
                }
                Ok(None) => Ok(()),
                Err(e) => Err(capnp::Error::failed(e.to_string())),
            }
        }

        #[async_backtrace::framed]
        async fn kill(&self, _: KillParams, _: KillResults) -> Result<(), capnp::Error> {
            self.borrow_mut().cancellation_token.cancel();
            match self.borrow_mut().child.kill().await {
                Ok(_) => Ok(()),
                Err(e) => Err(capnp::Error::failed(e.to_string())),
            }
        }

        #[async_backtrace::framed]
        async fn get_api(
            &self,
            _params: GetApiParams,
            mut results: GetApiResults,
        ) -> Result<(), capnp::Error> {
            results
                .get()
                .set_api(capnp_rpc::new_client(self.borrow().stdin.clone()))
        }

        #[async_backtrace::framed]
        async fn join(&self, _: JoinParams, mut results: JoinResults) -> Result<(), capnp::Error> {
            let results_builder = results.get();
            let mut process_error_builder = results_builder.init_result();

            match self.borrow_mut().child.wait().await {
                Ok(exitstatus) => {
                    // TODO: use std::os::unix::process::ExitStatusExt on unix to handle None
                    let exitcode = exitstatus.code().unwrap_or(0);
                    if exitcode != 0 {
                        process_error_builder.set_error_code(exitcode.into());
                    }

                    Ok(())
                }
                Err(e) => Err(capnp::Error::failed(e.to_string())),
            }
        }
    }

    pub struct PosixProgramImpl {
        program: File,
    }

    impl PosixProgramImpl {
        pub fn new(handle: u64) -> Self {
            unsafe {
                Self {
                    program: File::from_raw_filelike(handle as RawFilelike),
                }
            }
        }
        pub fn new_std(file: std::fs::File) -> Self {
            Self {
                program: cap_std::fs::File::from_std(file),
            }
        }
    }

    impl program::Server<PosixArgs, ByteStream, PosixError> for PosixProgramImpl {
        #[async_backtrace::framed]
        async fn spawn(
            &self,
            params: SpawnParams<PosixArgs, ByteStream, PosixError>,
            mut results: SpawnResults<PosixArgs, ByteStream, PosixError>,
        ) -> Result<(), ::capnp::Error> {
            let params_reader = params.get()?;

            let args = params_reader.get_args()?;
            let stdout: ByteStreamClient = args.get_stdout()?;
            let stderr: ByteStreamClient = args.get_stderr()?;
            let argv: capnp::text_list::Reader = args.get_args()?;
            let argv_iter = argv.into_iter().map(|item| match item {
                Ok(i) => Ok(i
                    .to_str()
                    .map_err(|e| capnp::Error::failed(e.to_string()))?),
                Err(e) => Err(e),
            });

            match PosixProcessImpl::spawn_process(&self.program, argv_iter, stdout, stderr) {
                Err(e) => Err(capnp::Error::failed(e.to_string())),
                Ok(process_impl) => {
                    let server_pointer = Rc::new(RefCell::new(process_impl));
                    let process_client: PosixProcessClient = capnp_rpc::new_client(server_pointer);
                    results.get().set_result(process_client);
                    Ok(())
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{PosixProgramClient, PosixProgramImpl};
        use crate::byte_stream::ByteStreamImpl;
        use cap_std::io_lifetimes::{FromFilelike, IntoFilelike};
        use std::fs::File;
        use tokio::task;

        #[tokio::test]
        #[async_backtrace::framed]
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
            };
            let spawn_process_client: PosixProgramClient =
                capnp_rpc::new_client(spawn_process_server);

            let e = task::LocalSet::new()
                .run_until(async_backtrace::location!().frame(async {
                    // Setting up stuff needed for RPC
                    let stdout_server = ByteStreamImpl::new(|bytes| {
                        println!("remote stdout: {}", std::str::from_utf8(bytes).unwrap());
                        std::future::ready(Ok(()))
                    });
                    let stdout_client: super::ByteStreamClient =
                        capnp_rpc::new_client(stdout_server);
                    let stderr_server = ByteStreamImpl::new(|bytes| {
                        println!("remote stderr: {}", std::str::from_utf8(bytes).unwrap());
                        std::future::ready(Ok(()))
                    });
                    let stderr_client: super::ByteStreamClient =
                        capnp_rpc::new_client(stderr_server);

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
                }))
                .await;

            e.unwrap();
        }
    }
}
