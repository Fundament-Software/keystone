pub mod unix_process {
    use std::process::{Stdio, ExitStatus};
    use capnp::capability::Promise;
    use capnp_rpc::pry;
    use tokio::io::AsyncRead;
    use tokio::process::Child;
    use tokio::process::ChildStdin;
    use tokio::task::{JoinHandle, spawn_local};
    use tokio_util::sync::CancellationToken;
    use crate::spawn_capnp::process;
    use crate::spawn_capnp::service_spawn;
    use crate::unix_process_capnp::{
        unix_process_api::Owned as UnixProcessApi,
        unix_process_args::Owned as UnixProcessArgs,
        unix_process_error::Owned as UnixProcessError
    };
    use crate::byte_stream_capnp::byte_stream::Client as ByteStreamClient;
    use tokio::process::Command;

    pub struct UnixProcessImpl {
        pub cancellation_token: CancellationToken,
        stdin: Option<ChildStdin>,
        pub stdout_task: JoinHandle<anyhow::Result<Option<usize>>>,
        pub stderr_task: JoinHandle<anyhow::Result<Option<usize>>>,
        pub process_task: JoinHandle<anyhow::Result<ExitStatus>>,
    }

    /// Helper function for UnixProcessImpl::spawn_process.
    /// Creates a task on the localset that reads the `AsyncRead` (usually a
    /// ChildStdout/ChildStderr) and copies to the ByteStreamClient.
    ///
    /// **Warning**: This function uses [spawn_local] and must be called in a LocalSet context.
    fn spawn_iostream_task(iostream: Option<impl AsyncRead + Unpin + 'static>, // In this case, 'static means an owned type. Also required for spawn_local
                           bytestream: ByteStreamClient,
                           cancellation_token: CancellationToken) -> JoinHandle<anyhow::Result<Option<usize>>> {
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

    /// **Warning**: This function uses [spawn_local] and must be called in a LocalSet context.
    fn spawn_process_task(mut child: Child, cancellation_token: CancellationToken) -> JoinHandle<anyhow::Result<ExitStatus>> {
        spawn_local(async move {
            tokio::select! {
                child_result = child.wait() => child_result.map_err(anyhow::Error::msg),
                () = cancellation_token.cancelled() => {
                    let kill_result = child.kill().await;
                    
                    if let Err(e) = kill_result {
                        Err(anyhow::Error::msg(e))
                    } else {
                        // Should should return immediately since kill calls it
                        child.wait().await.map_err(anyhow::Error::msg)
                    }
                }
            }
        })
    }

    impl UnixProcessImpl {
        /// Creates a new instance of `UnixProcessImpl` by spawning a child process.
        ///
        /// **Warning**: This function uses [spawn_local] and must be called in a LocalSet context.
        fn spawn_process<'i, I>(program: &str,
                                args_iter: I,
                                stdout_stream: ByteStreamClient,
                                stderr_stream: ByteStreamClient)
        -> anyhow::Result<UnixProcessImpl>
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

            // The almighty process task. 
            let process_task = spawn_process_task(child, cancellation_token.child_token());

            anyhow::Ok(UnixProcessImpl { cancellation_token, stdin, stdout_task, stderr_task, process_task })
        }
    }

    type GetApiParams = process::GetapiParams<UnixProcessApi, UnixProcessError>;
    type GetApiResults = process::GetapiResults<UnixProcessApi, UnixProcessError>;
    type GetErrorParams = process::GeterrorParams<UnixProcessApi, UnixProcessError>;
    type GetErrorResults = process::GeterrorResults<UnixProcessApi, UnixProcessError>;
    type KillParams = process::KillParams<UnixProcessApi, UnixProcessError>;
    type KillResults = process::KillResults<UnixProcessApi, UnixProcessError>;
    type UnixProcessClient = process::Client<UnixProcessApi, UnixProcessError>;
    
    impl process::Server<UnixProcessApi, UnixProcessError> for UnixProcessImpl {
        fn geterror(&mut self, params: GetErrorParams, results: GetErrorResults) -> Promise<(), capnp::Error> {
            Promise::ok(())
        }
    }

    pub struct UnixProcessServiceSpawnImpl ();

    type SpawnParams = service_spawn::SpawnParams<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError>;
    type SpawnResults = service_spawn::SpawnResults<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError>;

    impl service_spawn::Server<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError> for UnixProcessServiceSpawnImpl {
        fn spawn(&mut self, params: SpawnParams, mut results: SpawnResults) -> Promise<(), capnp::Error> {
            let params_reader = pry!(params.get());
            
            let program = pry!(params_reader.get_program());
            
            let args = pry!(params_reader.get_args());
            let stdout: ByteStreamClient = pry!(args.get_stdout());
            let stderr: ByteStreamClient = pry!(args.get_stderr());
            let argv: capnp::text_list::Reader = pry!(args.get_argv());

            match UnixProcessImpl::spawn_process(program, argv.into_iter(), stdout, stderr) {
                Err(e) => Promise::err(capnp::Error::failed(e.to_string())),
                Ok(process_impl) => {
                    let process_client: UnixProcessClient = capnp_rpc::new_client(process_impl);
                    results.get().set_result(process_client);
                    Promise::ok(())
                }
            }

        }
    }
}
