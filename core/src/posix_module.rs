pub mod posix_module {
    use crate::byte_stream::ByteStreamBufferImpl;
    use crate::byte_stream_capnp::byte_stream::Owned as ByteStream;
    use crate::keystone::KeystoneImpl;
    use crate::keystone_capnp::keystone;
    use crate::module_capnp::module_error;
    use crate::posix_module_capnp::posix_module;
    use crate::posix_spawn_capnp::posix_args::Owned as PosixArgs;
    use crate::posix_spawn_capnp::posix_error::Owned as PosixError;
    use crate::spawn_capnp::process;
    use crate::spawn_capnp::program;
    use crate::spawn_capnp::program::SpawnParams;
    use crate::spawn_capnp::program::SpawnResults;
    use capnp::any_pointer::Owned as any_pointer;
    use capnp::capability::FromClientHook;
    use capnp::private::capability::ClientHook;
    use capnp_macros::capnproto_rpc;
    use capnp_rpc::twoparty::VatNetwork;
    use capnp_rpc::Disconnector;
    use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
    use std::cell::RefCell;
    use std::rc::Rc;

    pub struct PosixModuleProcessImpl {
        posix_process: process::Client<ByteStream, PosixError>,
        //rpc_system: RpcSystem<rpc_twoparty_capnp::Side>,
        handle: tokio::task::JoinHandle<Result<(), capnp::Error>>,
        disconnector: Disconnector<rpc_twoparty_capnp::Side>,
        bootstrap: Box<dyn ClientHook>,
    }

    struct ClientExtraction(Box<dyn ClientHook>);

    impl FromClientHook for ClientExtraction {
        fn new(hook: Box<dyn ClientHook>) -> Self {
            Self(hook)
        }

        fn into_client_hook(self) -> Box<dyn ClientHook> {
            self.0
        }

        fn as_client_hook(&self) -> &dyn ClientHook {
            self.0.as_ref()
        }
    }

    impl process::Server<any_pointer, module_error::Owned<any_pointer>>
        for Rc<RefCell<PosixModuleProcessImpl>>
    {
        #[async_backtrace::framed]
        /// In this implementation of `spawn`, the functions returns the exit code of the child
        /// process.
        async fn get_error(
            &self,
            _: process::GetErrorParams<any_pointer, module_error::Owned<any_pointer>>,
            mut results: process::GetErrorResults<any_pointer, module_error::Owned<any_pointer>>,
        ) -> Result<(), capnp::Error> {
            Ok(())
        }

        #[async_backtrace::framed]
        async fn kill(
            &self,
            _: process::KillParams<any_pointer, module_error::Owned<any_pointer>>,
            _: process::KillResults<any_pointer, module_error::Owned<any_pointer>>,
        ) -> Result<(), capnp::Error> {
            Ok(())
        }

        #[async_backtrace::framed]
        async fn get_api(
            &self,
            _: process::GetApiParams<any_pointer, module_error::Owned<any_pointer>>,
            mut results: process::GetApiResults<any_pointer, module_error::Owned<any_pointer>>,
        ) -> Result<(), capnp::Error> {
            results
                .get()
                .init_api()
                .set_as_capability(self.borrow_mut().bootstrap.clone());
            Ok(())
        }
    }

    pub struct PosixModuleProgramImpl {
        posix_program: program::Client<PosixArgs, ByteStream, PosixError>,
    }

    impl program::Server<any_pointer, any_pointer, module_error::Owned<any_pointer>>
        for PosixModuleProgramImpl
    {
        #[async_backtrace::framed]
        async fn spawn(
            &self,
            _params: SpawnParams<any_pointer, any_pointer, module_error::Owned<any_pointer>>,
            mut results: SpawnResults<any_pointer, any_pointer, module_error::Owned<any_pointer>>,
        ) -> Result<(), ::capnp::Error> {
            let mut request = self.posix_program.spawn_request();
            let mut args = request.get().init_args();
            args.reborrow().init_args(0);

            // Here we create a bytestream implementation backed by a circular buffer. This is passed
            // into the new process so it can write to it, and then our RPC system reads from it.
            let stdout = ByteStreamBufferImpl::new();
            let stderr = ByteStreamBufferImpl::new();

            args.set_stdout(capnp_rpc::new_client(stdout.clone()));
            args.set_stderr(capnp_rpc::new_client(stderr.clone()));
            match request.send().promise.await {
                Ok(h) => {
                    let process = h.get()?.get_result()?;

                    match process.get_api_request().send().promise.await {
                        Ok(s) => {
                            let stdin = s.get()?.get_api()?;

                            let network = VatNetwork::new(
                                stdout.clone(), // read from the output stream of the process
                                stdin,          // write into the input stream of the process
                                rpc_twoparty_capnp::Side::Client,
                                Default::default(),
                            );

                            let keystone_client: keystone::Client =
                                capnp_rpc::new_client(KeystoneImpl {});
                            let mut rpc_system = RpcSystem::new(
                                Box::new(network),
                                Some(keystone_client.clone().client),
                            );

                            let disconnector = rpc_system.get_disconnector();
                            let bootstrap = rpc_system
                                .bootstrap::<ClientExtraction>(rpc_twoparty_capnp::Side::Server)
                                .into_client_hook();

                            let module_process = PosixModuleProcessImpl {
                                posix_process: process,
                                handle: tokio::task::spawn_local(
                                    async_backtrace::location!().frame(rpc_system),
                                ),
                                disconnector: disconnector,
                                bootstrap: bootstrap,
                            };

                            let module_process_client: process::Client<
                                any_pointer,
                                module_error::Owned<any_pointer>,
                            > = capnp_rpc::new_client(Rc::new(RefCell::new(module_process)));
                            results.get().set_result(module_process_client);
                            Ok(())
                        }
                        Err(e) => Err(capnp::Error::failed(e.to_string())),
                    }
                }
                Err(e) => Err(capnp::Error::failed(e.to_string())),
            }
        }
    }

    pub struct PosixModuleImpl {}

    #[capnproto_rpc(posix_module)]
    impl posix_module::Server for PosixModuleImpl {
        #[async_backtrace::framed]
        async fn wrap(&self, prog: Client) {
            let program = PosixModuleProgramImpl {
                posix_program: prog,
            };
            let program_client: program::Client<
                any_pointer,
                any_pointer,
                module_error::Owned<any_pointer>,
            > = capnp_rpc::new_client(program);
            results.get().set_result(program_client);
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{PosixModuleImpl, PosixModuleProgramImpl};
        use crate::byte_stream::ByteStreamBufferImpl;
        use crate::byte_stream::ByteStreamImpl;
        use crate::spawn::posix_process::PosixProgramImpl;
        use cap_std::io_lifetimes::{FromFilelike, IntoFilelike};
        use capnp::capability::FromClientHook;
        use std::cell::RefCell;
        use std::fs::File;
        use std::rc::Rc;
        use tokio::io::AsyncWriteExt;
        use tokio::task;
        use tokio_util::sync::CancellationToken;
        use tracing_subscriber::filter::LevelFilter;

        #[async_backtrace::framed]
        #[tokio::test]
        async fn test_raw_pipes() {
            #[cfg(windows)]
            let spawn_process_server = cap_std::fs::File::from_filelike(
                File::open("../target/debug/hello-world-module.exe")
                    .unwrap()
                    .into_filelike(),
            );
            #[cfg(not(windows))]
            let spawn_process_server = cap_std::fs::File::from_filelike(
                File::open("../target/debug/hello-world-module")
                    .unwrap()
                    .into_filelike(),
            );

            let args: Vec<Result<&str, ::capnp::Error>> = Vec::new();
            let mut child =
                crate::spawn::spawn_process_native(&spawn_process_server, args.into_iter())
                    .unwrap();

            let stdinref = Rc::new(RefCell::new(child.stdin.take().unwrap()));

            let stdin_stream_server = ByteStreamImpl::new(move |bytes| {
                let this_inner = stdinref.clone();
                let owned_bytes = bytes.to_owned();
                async move {
                    match this_inner.borrow_mut().write_all(&owned_bytes).await {
                        Ok(_) => Ok(()),
                        Err(e) => Err(capnp::Error::failed(e.to_string())),
                    }
                }
            });

            let stdinclient: crate::byte_stream_capnp::byte_stream::Client =
                capnp_rpc::new_client(stdin_stream_server);

            let cancellation_token = CancellationToken::new();

            let stdoutbuf = ByteStreamBufferImpl::new();
            let stdoutclient: crate::byte_stream_capnp::byte_stream::Client =
                capnp_rpc::new_client(stdoutbuf.clone());

            let mut stdout = child.stdout.take().unwrap();

            let network = capnp_rpc::twoparty::VatNetwork::new(
                stdoutbuf,   // read from the output stream of the process
                stdinclient, // write into the input stream of the process
                capnp_rpc::rpc_twoparty_capnp::Side::Client,
                Default::default(),
            );

            let keystone_client: super::keystone::Client =
                capnp_rpc::new_client(super::KeystoneImpl {});
            let mut rpc_system =
                super::RpcSystem::new(Box::new(network), Some(keystone_client.clone().client));

            let disconnector = rpc_system.get_disconnector();

            task::LocalSet::new()
                .run_until(async_backtrace::location!().frame(async {
                    let bootstrap = rpc_system
                        .bootstrap::<super::ClientExtraction>(
                            super::rpc_twoparty_capnp::Side::Server,
                        )
                        .into_client_hook();

                    tokio::task::spawn_local(async move {
                        tokio::select! {
                            _ = cancellation_token.cancelled() => Ok(None),
                            result = stdoutclient.copy(&mut stdout) => result.map(Some)
                        }
                    });

                    tokio::task::spawn_local(rpc_system);

                    let hello_world = crate::hello_world_capnp::hello_world::Client::new(bootstrap);

                    let mut request = hello_world.say_hello_request();
                    request.get().init_request().set_name("Keystone".into());

                    let reply = request.send().promise.await?;
                    let msg = reply.get()?.get_reply()?.get_message()?;

                    println!("Got reply! {}", msg.to_string()?);

                    Ok::<(), eyre::Error>(())
                }))
                .await
                .unwrap();
        }

        #[async_backtrace::framed]
        #[tokio::test]
        async fn test_process_creation() {
            //console_subscriber::init();

            tracing_subscriber::fmt()
                .with_max_level(LevelFilter::DEBUG)
                .with_target(true)
                .init();

            #[cfg(windows)]
            let spawn_process_server = PosixProgramImpl::new_std(
                File::open("../target/debug/hello-world-module.exe").unwrap(),
            );
            #[cfg(not(windows))]
            let spawn_process_server = PosixProgramImpl::new_std(
                File::open("../target/debug/hello-world-module").unwrap(),
            );

            let spawn_process_client: crate::spawn::posix_process::PosixProgramClient =
                capnp_rpc::new_client(spawn_process_server);

            let wrapper_server = PosixModuleImpl {};
            let wrapper_client: super::posix_module::Client = capnp_rpc::new_client(wrapper_server);

            let mut wrap_request = wrapper_client.wrap_request();
            wrap_request.get().set_prog(spawn_process_client);
            let wrap_response = wrap_request.send().promise.await.unwrap();
            let wrapped_client = wrap_response.get().unwrap().get_result().unwrap();

            let e = task::LocalSet::new()
                .run_until(async_backtrace::location!().frame(async {
                    let mut spawn_request = wrapped_client.spawn_request();
                    // TODO: Pass in the hello_world structural config parameters

                    let promise = spawn_request.send().promise;
                    let response = promise.await?;
                    let process_client = response.get()?.get_result()?;

                    let api_response = process_client.get_api_request().send().promise.await?;
                    let hello_client: crate::hello_world_capnp::hello_world::Client =
                        api_response.get()?.get_api()?.get_as_capability()?;

                    //tokio::time::sleep(tokio::time::Duration::from_millis(500));
                    let mut sayhello = hello_client.say_hello_request();
                    sayhello.get().init_request().set_name("Keystone".into());
                    let hello_response = sayhello.send().promise.await?;

                    let msg = hello_response.get()?.get_reply()?.get_message()?;

                    assert_eq!(msg, "Hello, Keystone!");

                    let geterror_response =
                        process_client.get_error_request().send().promise.await?;
                    let error_reader = geterror_response.get()?.get_result()?;

                    let error_message = match error_reader.which()? {
                        crate::module_capnp::module_error::Which::Backing(Ok(e)) => e
                            .get_as::<crate::posix_spawn_capnp::posix_error::Reader<'_>>()?
                            .get_error_message()?
                            .to_str()?,

                        crate::module_capnp::module_error::Which::ProtocolViolation(Ok(e)) => {
                            e.to_str()?
                        }

                        _ => "Error getting error!!!",
                    };
                    assert!(error_message.is_empty() == true);

                    Ok::<(), eyre::Error>(())
                }))
                .await;

            e.unwrap();
        }
    }
}