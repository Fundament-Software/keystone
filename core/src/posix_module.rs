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
    use crate::spawn_capnp::process;
    use crate::spawn_capnp::program;
    use crate::spawn_capnp::program::SpawnParams;
    use crate::spawn_capnp::program::SpawnResults;
    use capnp::any_pointer::Owned as any_pointer;
    use capnp::capability::Promise;
    use capnp_macros::capnproto_rpc;
    use capnp_rpc::twoparty::VatNetwork;
    use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
    use futures::AsyncRead;
    use std::cell::RefCell;
    use std::future::Future;
    use std::rc::Rc;

    pub struct PosixModuleProcessImpl {
        posix_process: process::Client<ByteStream, any_pointer>,
        //rpc_system: RpcSystem<rpc_twoparty_capnp::Side>,
        rpc_system: tokio::task::JoinHandle<Result<(), capnp::Error>>,
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
            capnp::ok()
        }
    }

    pub struct PosixModuleProgramImpl {
        posix_program: program::Client<PosixArgs, ByteStream, any_pointer>,
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

                args.set_stdout(capnp_rpc::new_client(stdout));
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
                                let module_process = PosixModuleProcessImpl {
                                    posix_process: process,
                                    rpc_system: tokio::task::spawn_local(RpcSystem::new(
                                        Box::new(network),
                                        Some(keystone_client.clone().client),
                                    )),
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
}
