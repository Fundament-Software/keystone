pub mod unix_process {
    use std::cell::RefCell;
    use std::convert::TryInto;
    use std::process::Stdio;
    use std::rc::Rc;
    use std::result::Result;
    use std::future::Future;
    use capnp::capability::Promise;
    use capnp_rpc::pry;
    use tokio::io::{AsyncRead, AsyncWriteExt};
    use tokio::process::Child;
    use tokio::process::ChildStdin;
    use tokio::task::{JoinHandle, spawn_local};
    use tokio_util::sync::CancellationToken;
    use crate::byte_stream::ByteStreamImpl;
    use crate::spawn_capnp::process;
    use crate::spawn_capnp::{service_spawn, service_spawn::Client};
    use crate::unix_process_capnp::{
        unix_process_api::Owned as UnixProcessApi,
        unix_process_args::Owned as UnixProcessArgs,
        unix_process_error::Owned as UnixProcessError,
    };
    use crate::byte_stream_capnp::byte_stream::Client as ByteStreamClient;
    use tokio::process::Command;

    pub struct UnixProcessImpl {
        pub cancellation_token: CancellationToken,
        stdin: Option<ChildStdin>,
        stdout_task: JoinHandle<eyre::Result<Option<usize>>>,
        stderr_task: JoinHandle<eyre::Result<Option<usize>>>,
        child: Child
    }

    /// Helper function for UnixProcessImpl::spawn_process.
    /// Creates a task on the localset that reads the `AsyncRead` (usually a
    /// ChildStdout/ChildStderr) and copies to the ByteStreamClient.
    ///
    /// **Warning**: This function uses [spawn_local] and must be called in a LocalSet context.
    fn spawn_iostream_task(iostream: Option<impl AsyncRead + Unpin + 'static>, // In this case, 'static means an owned type. Also required for spawn_local
                           bytestream: ByteStreamClient,
                           cancellation_token: CancellationToken) -> JoinHandle<eyre::Result<Option<usize>>> {
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

    impl UnixProcessImpl {
        fn new(cancellation_token: CancellationToken,
               stdin: Option<ChildStdin>,
               stdout_task: JoinHandle<eyre::Result<Option<usize>>>,
               stderr_task: JoinHandle<eyre::Result<Option<usize>>>,
               child: Child) -> Self {
            Self {
                cancellation_token,
                stdin,
                stdout_task,
                stderr_task,
                child
            }
        }

        /// Creates a new instance of `UnixProcessImpl` by spawning a child process.
        ///
        /// **Warning**: This function uses [spawn_local] and must be called in a LocalSet context.
        fn spawn_process<'i, I>(program: &str,
                                args_iter: I,
                                stdout_stream: ByteStreamClient,
                                stderr_stream: ByteStreamClient)
        -> eyre::Result<UnixProcessImpl>
        where
            I: IntoIterator<Item = Result<&'i str, capnp::Error>>
        {
            let args: Vec<&'i str> = args_iter.into_iter().collect::<Result<Vec<&'i str>, capnp::Error>>()?;

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
            let stdout_task = spawn_iostream_task(child.stdout.take(), stdout_stream, cancellation_token.child_token());
            let stderr_task = spawn_iostream_task(child.stderr.take(), stderr_stream, cancellation_token.child_token());

            eyre::Result::Ok(Self::new(cancellation_token, stdin, stdout_task, stderr_task, child))
        }
    }

    type GetApiParams = process::GetapiParams<UnixProcessApi, UnixProcessError>;
    type GetApiResults = process::GetapiResults<UnixProcessApi, UnixProcessError>;
    type GetErrorParams = process::GeterrorParams<UnixProcessApi, UnixProcessError>;
    type GetErrorResults = process::GeterrorResults<UnixProcessApi, UnixProcessError>;
    type KillParams = process::KillParams<UnixProcessApi, UnixProcessError>;
    type KillResults = process::KillResults<UnixProcessApi, UnixProcessError>;
    type UnixProcessClient = process::Client<UnixProcessApi, UnixProcessError>;
    
    // You might ask why this trait is implementated for an `Rc` + `RefCell` of `UnixProcessImpl`
    // Well thats a very good question
    impl process::Server<UnixProcessApi, UnixProcessError> for Rc<RefCell<UnixProcessImpl>> {
        /// In this implementation of `spawn`, the functions returns the exit code of the child
        /// process.
        fn geterror<'b>(&mut self, _: GetErrorParams, mut results: GetErrorResults) -> Result<impl Future<Output = Result<(), capnp::Error>> + 'b, capnp::Error> { 
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
                    },
                    Err(e) => Err(capnp::Error::failed(e.to_string()))
                }
            })
        }

        fn kill<'b>(&mut self, _: KillParams, _: KillResults) -> Result<impl Future<Output = Result<(), capnp::Error>> + 'b, capnp::Error> {
            let this = self.clone();
            Ok(async move {
                match this.borrow_mut().child.kill().await {
                    Ok(_) => Ok(()),
                    Err(e) => Err(capnp::Error::failed(e.to_string())),
                }
            })
        }

        fn getapi<'b>(&mut self, _: GetApiParams, mut results: GetApiResults) -> Result<impl Future<Output = Result<(), capnp::Error>> + 'b, capnp::Error> {
            let this = self.clone();
            let mut api_builder = results.get().init_api();

            let stdin_stream_server = ByteStreamImpl::new(move |bytes| {
                let this_inner = this.clone();
                let owned_bytes = bytes.to_owned();
                Promise::from_future(async move {
                    if let Some(mut stdin) = this_inner.borrow_mut().stdin.take() {
                        match stdin.write_all(&owned_bytes).await {
                            Ok(_) => Ok(()),
                            Err(e) => Err(capnp::Error::failed(e.to_string()))
                        }
                    } else {
                        Ok(())
                    }
                })
            });

            let stdin_stream_client: ByteStreamClient = capnp_rpc::new_client(stdin_stream_server);
            api_builder.set_stdin(stdin_stream_client);

            capnp::ok()
        }
    }

    pub struct UnixProcessServiceSpawnImpl ();

    type SpawnParams = service_spawn::SpawnParams<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError>;
    type SpawnResults = service_spawn::SpawnResults<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError>;

    impl service_spawn::Server<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError> for UnixProcessServiceSpawnImpl {
        fn spawn<'b>(&mut self, params: SpawnParams, mut results: SpawnResults) -> Result<impl Future<Output = Result<(), capnp::Error>> + 'b, capnp::Error> {
            let params_reader = params.get()?;
            
            let program = params_reader.get_program()?.to_str()?;
            
            let args = params_reader.get_args()?;
            let stdout: ByteStreamClient = args.get_stdout()?;
            let stderr: ByteStreamClient = args.get_stderr()?;
            let argv: capnp::text_list::Reader = args.get_argv()?;
            let argv_iter = argv.into_iter().map(|item| match item {
                Ok(i) => Ok(i.to_str().map_err(|_| capnp::Error::failed("Invalid utf-8 in argv".to_string()))?), 
                Err(e) => Err(e)
            });

            match UnixProcessImpl::spawn_process(program, argv_iter, stdout, stderr) {
                Err(e) => Err(capnp::Error::failed(e.to_string())),
                Ok(process_impl) => {
                    let server_pointer = Rc::new(RefCell::new(process_impl));
                    let process_client: UnixProcessClient = capnp_rpc::new_client(server_pointer);
                    results.get().set_result(process_client);
                    capnp::ok()
                }
            }
        }
    }

    pub type UnixProcessServiceSpawnClient = Client<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError>;
    pub type SpawnRequest = capnp::capability::Request<
        service_spawn::spawn_params::Owned<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError>,
        service_spawn::spawn_results::Owned<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError>>;
    
    impl UnixProcessServiceSpawnClient {
        /// Convenience function to create a new request to call `spawn_process` over RPC without
        /// sending it.
        pub fn build_spawn_request(&self, program: &str, argv: &[&str], stdout_stream: ByteStreamClient, stderr_stream: ByteStreamClient) -> eyre::Result<SpawnRequest> {
            let mut spawn_request = self.spawn_request();
            
            let mut params_builder = spawn_request.get();

            params_builder.set_program(program.into())?;

            let mut args_builder = params_builder.init_args();
            args_builder.set_stdout(stdout_stream);
            args_builder.set_stderr(stderr_stream);

            {
                let mut argv_builder = args_builder.init_argv(argv.len().try_into()?);
                for (idx, &x) in argv.iter().enumerate() {
                    argv_builder.reborrow().set(idx.try_into()?, x.into());
                }
            }

            Ok(spawn_request)
        }

        /// Convenience function to send a `spawn_process` call request over RPC.
        pub async fn send_spawn_request(spawn_request: SpawnRequest) -> Result<UnixProcessClient, capnp::Error> {
            let response = spawn_request.send().promise.await?;
            response.get()?.get_result()
        }

        /// Convenience function that creates and sends a request to call `spawn_process` over RPC.
        ///
        /// Equivalent to the composition of [build_spawn_request] and [send_spawn_request].
        pub async fn request_spawn_process(&self, 
                                           program: &str,
                                           argv: &[&str],
                                           stdout_stream: ByteStreamClient,
                                           stderr_stream: ByteStreamClient) -> eyre::Result<UnixProcessClient> {
            Ok(UnixProcessServiceSpawnClient::send_spawn_request(self.build_spawn_request(program, argv, stdout_stream, stderr_stream)?).await?)
        }
    }

    #[cfg(test)]
    mod tests {
        use capnp::capability::Promise;
        use tokio::task;
        use crate::byte_stream::ByteStreamImpl;
        use super::{UnixProcessServiceSpawnImpl, UnixProcessServiceSpawnClient};

        #[tokio::test]
        async fn test_process_creation() {
            let spawn_process_server = UnixProcessServiceSpawnImpl();
            let spawn_process_client: UnixProcessServiceSpawnClient = capnp_rpc::new_client(spawn_process_server);

            let e = task::LocalSet::new().run_until(async {
                // Setting up stuff needed for RPC
                let stdout_server = ByteStreamImpl::new(|bytes| {
                    println!("remote stdout: {}", std::str::from_utf8(bytes).unwrap());
                    Promise::ok(())
                });
                let stdout_client = capnp_rpc::new_client(stdout_server);
                let stderr_server = ByteStreamImpl::new(|bytes| {
                    println!("remote stderr: {}", std::str::from_utf8(bytes).unwrap());
                    Promise::ok(())
                });
                let stderr_client = capnp_rpc::new_client(stderr_server);

                let process_client = spawn_process_client.request_spawn_process(
                    "sh",
                    &["-c", r#"set; echo "Hello World!"; wait 10; exit 2"#],
                    stdout_client,
                    stderr_client).await?;

                let geterror_response = process_client.geterror_request().send().promise.await?;
                let error_reader = geterror_response.get()?.get_result()?;
                
                let error_code = error_reader.get_error_code();
                assert_eq!(error_code, 2);

                let error_message = error_reader.get_error_message()?;
                assert!(error_message.is_empty() == true);

                Ok::<(), eyre::Error>(())
            }).await;

            e.unwrap();
        }
    }
}
