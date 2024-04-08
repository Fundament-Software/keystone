pub mod posix_process {
    use crate::byte_stream::ByteStreamImpl;
    use crate::byte_stream_capnp::{
        byte_stream::Client as ByteStreamClient, byte_stream::Owned as ByteStream,
    };
    use crate::posix_spawn_capnp::{
        posix_args::Owned as PosixArgs, posix_error::Owned as PosixError,
    };
    use crate::spawn_capnp::program::{SpawnParams, SpawnResults};
    use crate::spawn_capnp::{process, program};
    use capnp::capability::Promise;
    use std::cell::RefCell;
    use std::convert::TryInto;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use std::rc::Rc;
    use std::result::Result;
    use std::str::FromStr;
    use tokio::io::{AsyncRead, AsyncWriteExt};
    use tokio::process::Child;
    use tokio::process::ChildStdin;
    use tokio::process::Command;
    use tokio::task::{spawn_local, JoinHandle};
    use tokio_util::sync::CancellationToken;

    type PosixProgramClient = program::Client<PosixArgs, ByteStream, PosixError>;

    pub struct PosixProcessImpl {
        pub cancellation_token: CancellationToken,
        stdin: Option<ChildStdin>,
        stdout_task: JoinHandle<eyre::Result<Option<usize>>>,
        stderr_task: JoinHandle<eyre::Result<Option<usize>>>,
        child: Child,
    }

    /// Helper function for PosixProcessImpl::spawn_process.
    /// Creates a task on the localset that reads the `AsyncRead` (usually a
    /// ChildStdout/ChildStderr) and copies to the ByteStreamClient.
    ///
    /// **Warning**: This function uses [spawn_local] and must be called in a LocalSet context.
    fn spawn_iostream_task(
        iostream: Option<impl AsyncRead + Unpin + 'static>, // In this case, 'static means an owned type. Also required for spawn_local
        bytestream: ByteStreamClient,
        cancellation_token: CancellationToken,
    ) -> JoinHandle<eyre::Result<Option<usize>>> {
        spawn_local(async move {
            if let Some(mut stream) = iostream {
                tokio::select! {
                    _ = cancellation_token.cancelled() => Ok(None),
                    result = bytestream.copy(&mut stream) => result.map(Some)
                }
            } else {
                Ok(None)
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
                stdin,
                stdout_task,
                stderr_task,
                child,
            }
        }

        /// Creates a new instance of `PosixProcessImpl` by spawning a child process.
        ///
        /// **Warning**: This function uses [spawn_local] and must be called in a LocalSet context.
        fn spawn_process<'i, I>(
            program: &str,
            args_iter: I,
            stdout_stream: ByteStreamClient,
            stderr_stream: ByteStreamClient,
        ) -> eyre::Result<PosixProcessImpl>
        where
            I: IntoIterator<Item = Result<&'i str, capnp::Error>>,
        {
            let args: Vec<&'i str> = args_iter
                .into_iter()
                .collect::<Result<Vec<&'i str>, capnp::Error>>()?;

            // Create the child process
            let mut child = Command::new(program)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .stdin(Stdio::piped())
                .spawn()?;

            // Stealing stdin also prevents the `child.wait()` call from closing it.
            let stdin = child.stdin.take();

            // Create a cancellation token to allow us to kill ("cancel") the process.
            let cancellation_token = CancellationToken::new();

            // Create the tasks to read stdout and stderr to the byte stream
            let stdout_task = spawn_iostream_task(
                child.stdout.take(),
                stdout_stream,
                cancellation_token.child_token(),
            );
            let stderr_task = spawn_iostream_task(
                child.stderr.take(),
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
    type PosixProcessClient = process::Client<ByteStream, PosixError>;

    // You might ask why this trait is implementated for an `Rc` + `RefCell` of `PosixProcessImpl`
    // Well thats a very good question
    impl process::Server<ByteStream, PosixError> for Rc<RefCell<PosixProcessImpl>> {
        /// In this implementation of `spawn`, the functions returns the exit code of the child
        /// process.
        fn get_error<'b>(
            &mut self,
            _: GetErrorParams,
            mut results: GetErrorResults,
        ) -> Result<impl Future<Output = Result<(), capnp::Error>> + 'b, capnp::Error> {
            let this = self.clone();
            Ok(async move {
                let results_builder = results.get();
                let mut process_error_builder = results_builder.init_result();

                match this.borrow_mut().child.wait().await {
                    Ok(exitstatus) => {
                        let exitcode = exitstatus.code().unwrap_or(i32::MIN);
                        process_error_builder.set_error_code(exitcode.into());
                        process_error_builder.set_error_message("".into());

                        Ok(())
                    }
                    Err(e) => Err(capnp::Error::failed(e.to_string())),
                }
            })
        }

        fn kill<'b>(
            &mut self,
            _: KillParams,
            _: KillResults,
        ) -> Result<impl Future<Output = Result<(), capnp::Error>> + 'b, capnp::Error> {
            let this = self.clone();
            Ok(async move {
                match this.borrow_mut().child.kill().await {
                    Ok(_) => Ok(()),
                    Err(e) => Err(capnp::Error::failed(e.to_string())),
                }
            })
        }

        fn get_api<'b>(
            &mut self,
            _: GetApiParams,
            mut results: GetApiResults,
        ) -> Result<impl Future<Output = Result<(), capnp::Error>> + 'b, capnp::Error> {
            let this = self.clone();

            let stdin_stream_server = ByteStreamImpl::new(move |bytes| {
                let this_inner = this.clone();
                let owned_bytes = bytes.to_owned();
                Promise::from_future(async move {
                    if let Some(mut stdin) = this_inner.borrow_mut().stdin.take() {
                        match stdin.write_all(&owned_bytes).await {
                            Ok(_) => Ok(()),
                            Err(e) => Err(capnp::Error::failed(e.to_string())),
                        }
                    } else {
                        Ok(())
                    }
                })
            });

            results
                .get()
                .set_api(capnp_rpc::new_client(stdin_stream_server))?;

            capnp::ok()
        }
    }

    pub struct PosixProgramImpl {
        program: PathBuf,
    }

    impl PosixProgramImpl {
        pub fn new(path: &str) -> Self {
            Self {
                program: PathBuf::from_str(path).unwrap(),
            }
        }
    }

    impl program::Server<PosixArgs, ByteStream, PosixError> for PosixProgramImpl {
        fn spawn<'a, 'b>(
            &'a mut self,
            params: SpawnParams<PosixArgs, ByteStream, PosixError>,
            mut results: SpawnResults<PosixArgs, ByteStream, PosixError>,
        ) -> Result<
            impl std::future::Future<Output = Result<(), ::capnp::Error>> + 'b,
            ::capnp::Error,
        > {
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

            match PosixProcessImpl::spawn_process(
                self.program.to_str().ok_or(capnp::Error::failed(
                    "Invalid utf-8 in program path".to_string(),
                ))?,
                argv_iter,
                stdout,
                stderr,
            ) {
                Err(e) => Err(capnp::Error::failed(e.to_string())),
                Ok(process_impl) => {
                    let server_pointer = Rc::new(RefCell::new(process_impl));
                    let process_client: PosixProcessClient = capnp_rpc::new_client(server_pointer);
                    results.get().set_result(process_client);
                    capnp::ok()
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::str::FromStr;

        use super::{PosixProgramClient, PosixProgramImpl};
        use crate::byte_stream::ByteStreamImpl;
        use capnp::capability::Promise;
        use tokio::task;

        #[tokio::test]
        async fn test_process_creation() {
            let spawn_process_server = PosixProgramImpl {
                #[cfg(windows)]
                program: std::path::PathBuf::from_str("cmd.exe").unwrap(),
                #[cfg(not(windows))]
                program: std::path::PathBuf::from_str("sh").unwrap(),
            };
            let spawn_process_client: PosixProgramClient =
                capnp_rpc::new_client(spawn_process_server);

            let e = task::LocalSet::new()
                .run_until(async {
                    // Setting up stuff needed for RPC
                    let stdout_server = ByteStreamImpl::new(|bytes| {
                        println!("remote stdout: {}", std::str::from_utf8(bytes).unwrap());
                        Promise::ok(())
                    });
                    let stdout_client: super::ByteStreamClient =
                        capnp_rpc::new_client(stdout_server);
                    let stderr_server = ByteStreamImpl::new(|bytes| {
                        println!("remote stderr: {}", std::str::from_utf8(bytes).unwrap());
                        Promise::ok(())
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

                    let geterror_response =
                        process_client.get_error_request().send().promise.await?;
                    let error_reader = geterror_response.get()?.get_result()?;

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
}
