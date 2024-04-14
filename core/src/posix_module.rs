pub mod posix_module {
    use std::collections::VecDeque;
    use std::io::Stdin;
    use std::ops::DerefMut;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    use crate::byte_stream::ByteStreamImpl;
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
    use capnp::capability::Promise;
    use capnp::private::capability::ClientHook;
    use capnp_macros::capnproto_rpc;
    use capnp_rpc::twoparty::VatNetwork;
    use capnp_rpc::Disconnector;
    use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
    use futures::AsyncRead;
    use std::cell::RefCell;
    use std::future::Future;
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
        /// In this implementation of `spawn`, the functions returns the exit code of the child
        /// process.
        fn get_error<'b>(
            &mut self,
            _: process::GetErrorParams<any_pointer, module_error::Owned<any_pointer>>,
            mut results: process::GetErrorResults<any_pointer, module_error::Owned<any_pointer>>,
        ) -> Result<impl Future<Output = Result<(), capnp::Error>> + 'b, capnp::Error> {
            capnp::ok()
        }

        fn kill<'b>(
            &mut self,
            _: process::KillParams<any_pointer, module_error::Owned<any_pointer>>,
            _: process::KillResults<any_pointer, module_error::Owned<any_pointer>>,
        ) -> Result<impl Future<Output = Result<(), capnp::Error>> + 'b, capnp::Error> {
            capnp::ok()
        }

        fn get_api<'b>(
            &mut self,
            _: process::GetApiParams<any_pointer, module_error::Owned<any_pointer>>,
            mut results: process::GetApiResults<any_pointer, module_error::Owned<any_pointer>>,
        ) -> Result<impl Future<Output = Result<(), capnp::Error>> + 'b, capnp::Error> {
            results
                .get()
                .init_api()
                .set_as_capability(self.borrow_mut().bootstrap.clone());
            capnp::ok()
        }
    }

    pub struct PosixModuleProgramImpl {
        posix_program: program::Client<PosixArgs, ByteStream, PosixError>,
    }

    pub struct ByteStreamBuffer {
        buf: Vec<u8>,
        pending: AtomicUsize,
    }

    #[derive(Clone)]
    pub struct ByteStreamBufferRef(Rc<RefCell<ByteStreamBuffer>>);

    impl Future for ByteStreamBufferRef {
        type Output = ();

        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            if self
                .0
                .borrow()
                .pending
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
            {
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(())
            }
        }
    }

    impl AsyncRead for ByteStreamBufferRef {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            let r = self.0.borrow();
            let pending = r.pending.load(std::sync::atomic::Ordering::Acquire);
            if pending > 0 {
                let start = r.buf.len() - pending;
                let len = std::cmp::min(pending, buf.len());
                buf[0..len].copy_from_slice(&r.buf[start..(start + len)]);
                r.pending
                    .fetch_sub(len, std::sync::atomic::Ordering::Release);
                std::task::Poll::Ready(Ok(len))
            } else {
                std::task::Poll::Pending
            }
        }
    }
    impl program::Server<any_pointer, any_pointer, module_error::Owned<any_pointer>>
        for PosixModuleProgramImpl
    {
        fn spawn<'a, 'b>(
            &'a mut self,
            params: SpawnParams<any_pointer, any_pointer, module_error::Owned<any_pointer>>,
            mut results: SpawnResults<any_pointer, any_pointer, module_error::Owned<any_pointer>>,
        ) -> Result<impl Future<Output = Result<(), ::capnp::Error>> + 'b, ::capnp::Error> {
            let mut request = self.posix_program.spawn_request();
            Ok(async move {
                let mut args = request.get().init_args();
                args.reborrow().init_args(0);

                // Here we create a bytestream implementation backed by a circular buffer. This is passed
                // into the new process so it can write to it, and then our RPC system reads from it.
                let stdoutbuf = ByteStreamBufferRef(Rc::new(RefCell::new(ByteStreamBuffer {
                    buf: Vec::new(),
                    pending: AtomicUsize::default(),
                })));

                let stdout = {
                    let stdoutbuf = stdoutbuf.clone();
                    ByteStreamImpl::new(move |bytes| {
                        let stdoutref = stdoutbuf.clone();
                        let owned_bytes = bytes.to_owned();
                        Promise::from_future(async move {
                            stdoutref.clone().await;
                            let binding = stdoutref.clone();
                            let mut r = binding.0.borrow_mut();
                            r.buf = owned_bytes;
                            r.pending
                                .store(r.buf.len(), std::sync::atomic::Ordering::Release);
                            Ok(())
                        })
                    })
                };

                let stderrbuf = ByteStreamBufferRef(Rc::new(RefCell::new(ByteStreamBuffer {
                    buf: Vec::new(),
                    pending: AtomicUsize::default(),
                })));

                let stderr = {
                    let stderrbuf = stderrbuf.clone();
                    ByteStreamImpl::new(move |bytes| {
                        let stderrref = stderrbuf.clone();
                        let owned_bytes = bytes.to_owned();
                        Promise::from_future(async move {
                            stderrref.clone().await;
                            let binding = stderrref.clone();
                            let mut r = binding.0.borrow_mut();
                            r.buf = owned_bytes;
                            r.pending
                                .store(r.buf.len(), std::sync::atomic::Ordering::Release);
                            Ok(())
                        })
                    })
                };

                args.set_stdout(capnp_rpc::new_client(stdout));
                args.set_stderr(capnp_rpc::new_client(stderr));
                match request.send().promise.await {
                    Ok(h) => {
                        let process = h.get()?.get_result()?;

                        let stdoutbuf = stdoutbuf.clone();
                        match process.get_api_request().send().promise.await {
                            Ok(s) => {
                                let stdin = s.get()?.get_api()?;

                                let network = VatNetwork::new(
                                    stdoutbuf.clone(), // read from the output stream of the process
                                    stdin,             // write into the input stream of the process
                                    rpc_twoparty_capnp::Side::Server,
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
                                    .bootstrap::<ClientExtraction>(rpc_twoparty_capnp::Side::Client)
                                    .into_client_hook();
                                let module_process = PosixModuleProcessImpl {
                                    posix_process: process,
                                    handle: tokio::task::spawn_local(rpc_system),
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
            })
        }
    }

    pub struct PosixModuleImpl {}

    #[capnproto_rpc(posix_module)]
    impl posix_module::Server for PosixModuleImpl {
        fn wrap(&mut self, prog: Client) {
            let program = PosixModuleProgramImpl {
                posix_program: prog,
            };
            let program_client: program::Client<
                any_pointer,
                any_pointer,
                module_error::Owned<any_pointer>,
            > = capnp_rpc::new_client(program);
            results.get().set_result(program_client);
            capnp::ok()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{PosixModuleImpl, PosixModuleProgramImpl};
        use crate::byte_stream::ByteStreamImpl;
        use crate::spawn::posix_process::PosixProgramImpl;
        use cap_std::io_lifetimes::{FromFilelike, IntoFilelike};
        use capnp::capability::Promise;
        use std::fs::File;
        use tokio::task;

        #[tokio::test]
        async fn test_process_creation() {
            println!("{}", std::env::current_dir().unwrap().to_string_lossy());
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
                .run_until(async {
                    let mut spawn_request = wrapped_client.spawn_request();
                    // TODO: Pass in the hello_world structural config parameters

                    let response = spawn_request.send().promise.await?;
                    let process_client = response.get()?.get_result()?;

                    let api_response = process_client.get_api_request().send().promise.await?;
                    let hello_client: crate::hello_world_capnp::hello_world::Client =
                        api_response.get()?.get_api()?.get_as_capability()?;

                    let mut sayhello = hello_client.say_hello_request();
                    sayhello.get().init_request().set_name("Keystone".into());
                    let hello_response = sayhello.send().promise.await?;
                    let msg = hello_response.get()?.get_reply()?.get_message()?;

                    println!("Got reply! {}", msg.to_string()?);

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
                })
                .await;

            e.unwrap();
        }
    }
}
