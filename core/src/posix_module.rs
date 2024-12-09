use crate::byte_stream::ByteStreamBufferImpl;
use crate::byte_stream_capnp::byte_stream::Owned as ByteStream;
use crate::keystone::CapnpResult;
use crate::module_capnp::module_error;
use crate::module_capnp::module_start;
use crate::posix_module_capnp::posix_module;
use crate::posix_module_capnp::posix_module_args;
use crate::posix_spawn_capnp::posix_args::Owned as PosixArgs;
use crate::posix_spawn_capnp::posix_error::Owned as PosixError;
use crate::spawn_capnp::process;
use crate::spawn_capnp::program;
use capnp::any_pointer::Owned as any_pointer;
use capnp::any_pointer::Owned as cap_pointer;
use capnp::capability::FromServer;
use capnp::capability::RemotePromise;
use capnp_macros::capnproto_rpc;
use capnp_rpc::CapabilityServerSet;
use eyre::Context;
use futures_util::future::LocalBoxFuture;
use futures_util::FutureExt;
use std::process::ExitStatus;
use std::{cell::RefCell, rc::Rc};
use tokio::sync::mpsc;

pub struct PosixModuleProcessImpl {
    posix_process: process::Client<ByteStream, PosixError>,
    pub(crate) rpc_future: Option<LocalBoxFuture<'static, eyre::Result<()>>>,
    pub(crate) bootstrap: Option<module_start::Client<any_pointer, cap_pointer>>,
    api: RemotePromise<module_start::start_results::Owned<any_pointer, cap_pointer>>,
    pub(crate) debug_name: Option<String>,
    pub(crate) pause: mpsc::Sender<bool>,
    //pub(crate) stderr: ByteStreamBufferImpl,
}

type AnyPointerClient = process::Client<cap_pointer, module_error::Owned<any_pointer>>;
pub type ModuleProcessCapSet =
    CapabilityServerSet<RefCell<PosixModuleProcessImpl>, AnyPointerClient>;

impl process::Server<cap_pointer, module_error::Owned<any_pointer>>
    for RefCell<PosixModuleProcessImpl>
{
    /// In this implementation of `spawn`, the functions returns the exit code of the child
    /// process.
    async fn get_error(
        &self,
        _: process::GetErrorParams<cap_pointer, module_error::Owned<any_pointer>>,
        mut results: process::GetErrorResults<cap_pointer, module_error::Owned<any_pointer>>,
    ) -> Result<(), capnp::Error> {
        let span = tracing::trace_span!("posix_module_process");
        let _enter = span.enter();
        tracing::trace!("get_error()");
        let request = self.borrow().posix_process.get_error_request();
        let posix_err = request.send().promise.await?;
        let builder = results.get().init_result();
        builder
            .init_backing()
            .set_as(posix_err.get()?.get_result()?)?;
        Ok(())
    }

    async fn kill(
        &self,
        _: process::KillParams<cap_pointer, module_error::Owned<any_pointer>>,
        _: process::KillResults<cap_pointer, module_error::Owned<any_pointer>>,
    ) -> Result<(), capnp::Error> {
        let span = tracing::trace_span!("posix_module_process");
        let _enter = span.enter();
        tracing::trace!("kill()");
        let request = self.borrow().posix_process.kill_request();
        request.send().promise.await?;
        Ok(())
    }

    async fn get_api(
        &self,
        _: process::GetApiParams<cap_pointer, module_error::Owned<any_pointer>>,
        mut results: process::GetApiResults<cap_pointer, module_error::Owned<any_pointer>>,
    ) -> Result<(), capnp::Error> {
        let span = tracing::trace_span!("posix_module_process");
        let _enter = span.enter();
        tracing::trace!("get_api()");
        results
            .get()
            .init_api()
            .set_as_capability(self.borrow().api.pipeline.get_api().as_cap());
        Ok(())
    }

    async fn join(
        &self,
        _: process::JoinParams<cap_pointer, module_error::Owned<any_pointer>>,
        mut results: process::JoinResults<cap_pointer, module_error::Owned<any_pointer>>,
    ) -> Result<(), ::capnp::Error> {
        let span = tracing::trace_span!("posix_module_process");
        let _enter = span.enter();
        tracing::trace!("join()");
        let request = self.borrow().posix_process.join_request();
        let posix_err = request.send().promise.await?;
        let builder = results.get().init_result();
        builder
            .init_backing()
            .set_as(posix_err.get()?.get_result()?)?;
        Ok(())
    }
}

pub struct PosixModuleProgramImpl<
    Fut: std::future::Future<Output = eyre::Result<()>> + 'static,
    F: Fn(ByteStreamBufferImpl) -> Fut + Clone,
> {
    posix_program: program::Client<PosixArgs, ByteStream, PosixError>,
    module: PosixModuleImpl<Fut, F>,
}

// Flattens nested results into a capnp::Result
fn flatten_to_capnp<T, E: ToString, E2: ToString>(
    result: Result<Result<T, E>, E2>,
) -> capnp::Result<T> {
    match result {
        Ok(Ok(r)) => Ok(r),
        Ok(Err(e)) => Err(e).to_capnp(),
        Err(e) => Err(e).to_capnp(),
    }
}

#[capnproto_rpc(program)]
impl<
        Fut: std::future::Future<Output = eyre::Result<()>> + 'static,
        F: Fn(ByteStreamBufferImpl) -> Fut + Clone,
    >
    program::Server<
        posix_module_args::Owned<any_pointer>,
        cap_pointer,
        module_error::Owned<any_pointer>,
    > for PosixModuleProgramImpl<Fut, F>
{
    async fn spawn(&self, args: Reader) -> Result<(), ::capnp::Error> {
        let span = tracing::span!(tracing::Level::DEBUG, "posix_program::spawn");
        let _enter = span.enter();
        let mut request = self.posix_program.spawn_request();
        let mut prog_args = request.get().init_args();
        prog_args.reborrow().init_args(0);

        // Here we create a bytestream implementation backed by a circular buffer. This is passed
        // into the new process so it can write to it, and then our RPC system reads from it.
        let stdout = ByteStreamBufferImpl::new();
        let stderr = ByteStreamBufferImpl::new();

        prog_args.set_stdout(capnp_rpc::new_client(stdout.clone()));
        prog_args.set_stderr(capnp_rpc::new_client(stderr.clone()));

        match request.send().promise.await {
            Ok(h) => {
                let process = h.get()?.get_result()?;

                match process.get_api_request().send().promise.await {
                    Ok(s) => {
                        let stdin = crate::byte_stream::ClientWriter::new(s.get()?.get_api()?);

                        let pair = args;

                        // read from the output stream of the process
                        // write into the input stream of the process
                        let (mut rpc_system, bootstrap, disconnector, api) =
                            crate::keystone::init_rpc_system(
                                stdout.clone(),
                                stdin,
                                self.module.host.clone().client,
                                pair.get_config()?,
                            )?;

                        let (process_future, killsender) = {
                            let process_impl = self
                                .module
                                .process_set
                                .borrow_mut()
                                .get_local_server_of_resolved(&process)
                                .ok_or(capnp::Error::failed(
                                    "Couldn't get process implementation!".into(),
                                ))?;

                            (
                                process_impl
                                    .future
                                    .take()
                                    .expect("Process had no future to wait on!"),
                                process_impl
                                    .killsender
                                    .take()
                                    .expect("Process had no killsender!"),
                            )
                        };

                        let sink = (self.module.stderr_sink)(stderr.clone());

                        let mut process_handle = tokio::task::spawn_local(async move {
                            tokio::try_join!(process_future, sink).map(|(f, _)| f)
                        });

                        let (send, mut pause_recv) = mpsc::channel::<bool>(1);

                        let module_process = Rc::new_cyclic(|this| {
                            let this = this.clone();
                            let bootstrap_hook = bootstrap.client.hook.add_ref();
                            let fut = async move {
                                let kill = killsender;
                                let disconnect = disconnector;
                                {
                                    // We must be exceedingly careful that the async block itself only borrows the weak reference, so
                                    // that when we upgrade it here, it will go out of scope after we're finished and not cause a cycle.
                                    let handle: Rc<
                                        process::ServerDispatch<
                                            RefCell<PosixModuleProcessImpl>,
                                            any_pointer,
                                            module_error::Owned<any_pointer>,
                                        >,
                                    > = this.upgrade().unwrap();

                                    let mut rpc_handle = tokio::task::spawn_local(async move {
                                        loop {
                                            // If the RPC system, finishes, break out of the loop. If we recieve a "go ahead" signal, continue the loop. Otherwise, pause the RPC system
                                            tokio::select! { r = &mut rpc_system => break r, q = pause_recv.recv() => if q.unwrap_or(false) { () } else { continue; }, }
                                            // Wait until we recieve a false value from the signaler
                                            while pause_recv.recv().await.unwrap_or(false) {}
                                        }
                                    });

                                    // Here, we await both joinhandles to see which returns first. If one returns an error, we kill the other one, otherwise
                                    // we await the other handle. This is done carefully so that we never await a handle twice

                                    let result = tokio::select! {
                                        r = &mut rpc_handle => {
                                            if let Err(e) = flatten_to_capnp(r) {
                                                let _ = kill.send(()); Err(e)
                                            } else {
                                                flatten_to_capnp(process_handle.await)
                                            }
                                        },
                                        r = &mut process_handle => {
                                            let blah: capnp::Result<ExitStatus> = match r {
                                                Ok(Ok(e)) => {
                                                    if e.success() {
                                                        flatten_to_capnp(rpc_handle.await).map(move |_| e)
                                                    } else {
                                                        let _ = disconnect.await;
                                                        Err(e).to_capnp()
                                                    }
                                                }
                                                Ok(Err(e)) => {
                                                    let _ = disconnect.await;
                                                    Err(e).to_capnp()
                                                }
                                                Err(e) => {
                                                    let _ = disconnect.await;
                                                    Err(e).to_capnp()
                                                }
                                            };
                                            blah
                                         },
                                    };

                                    result.map(|_| ()).wrap_err_with(move || {
                                        let name = handle.borrow().debug_name.clone();
                                        if let Some(n) = name {
                                            format!("Error from {}", n)
                                        } else {
                                            format!(
                                                "Error from Unknown Module {}",
                                                bootstrap_hook.get_brand()
                                            )
                                        }
                                    })
                                }
                            };

                            AnyPointerClient::from_server(RefCell::new(PosixModuleProcessImpl {
                                posix_process: process,
                                rpc_future: Some(fut.boxed_local()),
                                bootstrap: Some(bootstrap),
                                api,
                                debug_name: None,
                                pause: send,
                            }))
                        });

                        let module_process_client: AnyPointerClient = self
                            .module
                            .module_process_set
                            .borrow_mut()
                            .from_rc(module_process);
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

pub struct PosixModuleImpl<
    Fut: std::future::Future<Output = eyre::Result<()>> + 'static,
    F: (Fn(ByteStreamBufferImpl) -> Fut) + Clone,
> {
    pub host: crate::keystone_capnp::host::Client<any_pointer>,
    pub module_process_set: Rc<RefCell<ModuleProcessCapSet>>,
    pub process_set: Rc<RefCell<crate::posix_process::ProcessCapSet>>,
    pub stderr_sink: F,
}

impl<
        Fut: std::future::Future<Output = eyre::Result<()>> + 'static,
        F: (Fn(ByteStreamBufferImpl) -> Fut) + Clone,
    > Clone for PosixModuleImpl<Fut, F>
{
    fn clone(&self) -> Self {
        Self {
            host: self.host.clone(),
            module_process_set: self.module_process_set.clone(),
            process_set: self.process_set.clone(),
            stderr_sink: self.stderr_sink.clone(),
        }
    }
}

#[capnproto_rpc(posix_module)]
impl<
        Fut: std::future::Future<Output = eyre::Result<()>> + 'static,
        F: (Fn(ByteStreamBufferImpl) -> Fut) + 'static + Clone,
    > posix_module::Server for PosixModuleImpl<Fut, F>
{
    async fn wrap(&self, prog: Client) {
        let span = tracing::span!(tracing::Level::DEBUG, "posix_module::wrap");
        let _enter = span.enter();
        let program = PosixModuleProgramImpl::<Fut, F> {
            posix_program: prog,
            module: self.clone(),
        };
        let program_client: program::Client<
            posix_module_args::Owned<any_pointer>,
            cap_pointer,
            module_error::Owned<any_pointer>,
        > = capnp_rpc::new_client(program);
        results.get().set_result(program_client);
        Ok(())
    }
}
