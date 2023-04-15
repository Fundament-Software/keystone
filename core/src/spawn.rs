pub mod unix_process {
    use std::process::{Stdio, ExitStatus};
    use capnp::capability::Promise;
    use capnp_rpc::pry;
    use tokio::process::ChildStdin;
    use tokio::task::JoinHandle;
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
        cancellation_token: CancellationToken,
        stdin: Option<ChildStdin>,
        stdout_task: JoinHandle<()>,
        stderr_task: JoinHandle<()>,
        process_task: JoinHandle<()>
        //process_task: JoinHandle<anyhow::Result<ExitStatus>>,
    }

    impl UnixProcessImpl {
        fn spawn_process<'i, I>(program: &str, args_iter: I, stdout_stream: ByteStreamClient, stderr_stream: ByteStreamClient) -> anyhow::Result<UnixProcessImpl>
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
           
            // Create the task to read stdout to the byte stream
            let stdout = child.stdout.take();
            let stdout_token = cancellation_token.child_token();
            let stdout_task = tokio::spawn(async move {
                if let Some(mut stream) = stdout {
                    tokio::select! {
                        _ = stdout_token.cancelled() => (),
                        _ = stdout_stream.copy(&mut stream) => ()
                    }
                }
            });

            // Create the task to read stderr to the byte stream
            let stderr = child.stderr.take();
            let stderr_token = cancellation_token.child_token();
            let stderr_task = tokio::spawn(async move {
                if let Some(mut stream) = stderr {
                    tokio::select! {
                        _ = stderr_token.cancelled() => (),
                        _ = stderr_stream.copy(&mut stream) => ()
                    }
                }
            });

            // The almighty process task. 
            let proccess_token = cancellation_token.child_token();
            let process_task = tokio::spawn(async move {
                tokio::select! {
                    _ = child.wait() => (),
                    _ = proccess_token.cancelled() => { child.kill().await; }
                } 
            });

            anyhow::Ok(UnixProcessImpl { cancellation_token, stdin, stdout_task, stderr_task, process_task })
        }
    }

    type GetApiParams = process::GetapiParams<UnixProcessApi, UnixProcessError>;
    type GetApiResults = process::GetapiResults<UnixProcessApi, UnixProcessError>;
    type GetErrorParams = process::GeterrorParams<UnixProcessApi, UnixProcessError>;
    type GetErrorResults = process::GeterrorResults<UnixProcessApi, UnixProcessError>;
    type KillParams = process::KillParams<UnixProcessApi, UnixProcessError>;
    type KillResults = process::KillResults<UnixProcessApi, UnixProcessError>;
    
    impl process::Server<UnixProcessApi, UnixProcessError> for UnixProcessImpl {
        fn geterror(&mut self, params: GetErrorParams, results: GetErrorResults) -> Promise<(), capnp::Error> {
            Promise::ok(())
        }
    }

    pub struct UnixProcessServiceSpawnImpl ();

    type SpawnParams = service_spawn::SpawnParams<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError>;
    type SpawnResults = service_spawn::SpawnResults<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError>;

    impl service_spawn::Server<capnp::text::Owned, UnixProcessArgs, UnixProcessApi, UnixProcessError> for UnixProcessServiceSpawnImpl {
        fn spawn(&mut self, params: SpawnParams, results: SpawnResults) -> Promise<(), capnp::Error> {
            let params_reader = pry!(params.get());
            
            let program = pry!(params_reader.get_program());
            
            let args = pry!(params_reader.get_args());
            let stdout: ByteStreamClient = pry!(args.get_stdout());
            let stderr: ByteStreamClient = pry!(args.get_stderr());
            let argv: capnp::text_list::Reader = pry!(args.get_argv());

            Promise::ok(())
        }
    }
}
