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
use std::{cell::RefCell, rc::Rc};

pub struct PosixModuleProcessImpl {
    posix_process: process::Client<ByteStream, PosixError>,
    pub(crate) rpc_future: Option<LocalBoxFuture<'static, eyre::Result<()>>>,
    pub(crate) bootstrap: Option<module_start::Client<any_pointer, cap_pointer>>,
    api: RemotePromise<module_start::start_results::Owned<any_pointer, cap_pointer>>,
    pub(crate) debug_name: Option<String>,
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
        let request = self.borrow_mut().posix_process.get_error_request();
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
        let request = self.borrow_mut().posix_process.kill_request();
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
            .set_as_capability(self.borrow_mut().api.pipeline.get_api().as_cap());
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
        let request = self.borrow_mut().posix_process.join_request();
        let posix_err = request.send().promise.await?;
        let builder = results.get().init_result();
        builder
            .init_backing()
            .set_as(posix_err.get()?.get_result()?)?;
        Ok(())
    }
}

pub struct PosixModuleProgramImpl {
    posix_program: program::Client<PosixArgs, ByteStream, PosixError>,
    module: PosixModuleImpl,
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
impl
    program::Server<
        posix_module_args::Owned<any_pointer>,
        cap_pointer,
        module_error::Owned<any_pointer>,
    > for PosixModuleProgramImpl
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
                        let (rpc_system, bootstrap, disconnector, api) =
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

                        let mut process_handle = tokio::task::spawn_local(process_future);

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

                                    let mut rpc_handle = tokio::task::spawn_local(rpc_system);

                                    // Here, we await both joinhandles to see which returns first. If one returns an error, we kill the other one, otherwise
                                    // we await the other handle. This is done carefully so that we never await a handle twice

                                    let result = tokio::select! {
                                        r = &mut rpc_handle => {
                                            if let Err(e) = flatten_to_capnp(r) {
                                                let _ = kill.send(()); Err(e)
                                            } else {
                                                process_handle.await.to_capnp()
                                            }
                                        },
                                        r = &mut process_handle => {
                                            if let Err(e) = r {
                                                let _ = disconnect.await;
                                                Err(e).to_capnp()
                                            } else {
                                                flatten_to_capnp(rpc_handle.await)
                                            }
                                         },
                                    };

                                    result.wrap_err_with(move || {
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

#[derive(Clone)]
pub struct PosixModuleImpl {
    pub host: crate::keystone_capnp::host::Client<any_pointer>,
    pub module_process_set: Rc<RefCell<ModuleProcessCapSet>>,
    pub process_set: Rc<RefCell<crate::posix_process::ProcessCapSet>>,
}

#[capnproto_rpc(posix_module)]
impl posix_module::Server for PosixModuleImpl {
    async fn wrap(&self, prog: Client) {
        let span = tracing::span!(tracing::Level::DEBUG, "posix_module::wrap");
        let _enter = span.enter();
        let program = PosixModuleProgramImpl {
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
/*
#[cfg(test)]
mod tests {
    use super::{any_pointer, PosixModuleImpl};
    use crate::byte_stream::ByteStreamBufferImpl;
    use crate::byte_stream::ByteStreamImpl;
    use crate::keystone::CellCapSet;
    use crate::posix_process::PosixProgramImpl;
    use cap_std::io_lifetimes::{FromFilelike, IntoFilelike};
    use std::cell::RefCell;
    use std::fs::File;
    use std::rc::Rc;
    use tempfile::NamedTempFile;
    use tokio::io::AsyncWriteExt;
    use tokio::task;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_raw_pipes() {
        let spawn_process_server = cap_std::fs::File::from_filelike(
            File::open(crate::keystone::get_binary_path("hello-world-module"))
                .unwrap()
                .into_filelike(),
        );
        let temp_db = NamedTempFile::new().unwrap().into_temp_path();

        let args: Vec<Result<&str, ::capnp::Error>> = Vec::new();
        let mut child =
            crate::posix_process::spawn_process_native(&spawn_process_server, args.into_iter())
                .unwrap();

        let stdinref = Rc::new(RefCell::new(child.stdin.take().unwrap()));

        let stdin_stream_server = ByteStreamImpl::new(move |bytes| {
            let this_inner = stdinref.clone();
            let owned_bytes = bytes.to_owned();
            async move {
                let mut stdin = this_inner.borrow_mut();
                match stdin.write_all(&owned_bytes).await {
                    Ok(_) => stdin
                        .flush()
                        .await
                        .to_capnp(),
                    Err(e) => Err(capnp::Error::failed(e.to_string())),
                }
            }
        });

        let stdinclient =
            crate::byte_stream::ClientWriter::new(capnp_rpc::new_client(stdin_stream_server));

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

        let db: crate::sqlite::SqliteDatabase = crate::database::Manager::open_database(
            std::path::Path::new(temp_db.as_os_str()),
            crate::database::OpenOptions::Create,
        )
        .unwrap();

        let keystone_client: crate::keystone_capnp::host::Client<any_pointer> =
            capnp_rpc::new_client(crate::host::HostImpl::new(
                0,
                Rc::new(RefCell::new(db)),
                Rc::new(RefCell::new(CellCapSet::new())),
            ));
        let mut rpc_system =
            capnp_rpc::RpcSystem::new(Box::new(network), Some(keystone_client.clone().client));

        let _ = rpc_system.get_disconnector();

        task::LocalSet::new()
            .run_until(async {
                let bootstrap: crate::module_capnp::module_start::Client<
                    hello_world::hello_world_capnp::config::Owned,
                    hello_world::hello_world_capnp::root::Owned,
                > = rpc_system.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server);

                tokio::task::spawn_local(async move {
                    tokio::select! {
                        _ = cancellation_token.cancelled() => Ok(None),
                        result = stdoutclient.copy(&mut stdout) => result.map(Some)
                    }
                });

                tokio::task::spawn_local(rpc_system);

                let start_request = bootstrap.start_request();
                //start_request.get().set_config()
                let root_response = start_request.send().promise.await?;
                let hello_world = root_response.get()?.get_api()?;

                let mut request = hello_world.say_hello_request();
                request.get().init_request().set_name("Keystone".into());

                let reply = request.send().promise.await?;
                let msg = reply.get()?.get_reply()?.get_message()?;

                println!("Got reply! {}", msg.to_string()?);

                Ok::<(), eyre::Error>(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_process_creation() {
        let process_set = Rc::new(RefCell::new(crate::posix_process::ProcessCapSet::new()));
        let spawn_process_server = PosixProgramImpl::new_std(
            File::open(crate::keystone::get_binary_path("hello-world-module")).unwrap(),
            process_set.clone(),
        );
        let spawn_process_client: crate::posix_process::PosixProgramClient =
            capnp_rpc::new_client(spawn_process_server);

        let temp_db = NamedTempFile::new().unwrap().into_temp_path();
        let db: crate::sqlite::SqliteDatabase = crate::database::Manager::open_database(
            std::path::Path::new(temp_db.as_os_str()),
            crate::database::OpenOptions::Create,
        )
        .unwrap();

        let wrapper_server = PosixModuleImpl {
            host: capnp_rpc::new_client(crate::host::HostImpl::new(
                0,
                Rc::new(RefCell::new(db)),
                Rc::new(RefCell::new(CellCapSet::new())),
            )),
            module_process_set: Rc::new(RefCell::new(super::ModuleProcessCapSet::new())),
            process_set: process_set,
        };
        let wrapper_client: super::posix_module::Client = capnp_rpc::new_client(wrapper_server);

        let mut wrap_request = wrapper_client.wrap_request();
        wrap_request.get().set_prog(spawn_process_client);
        let wrap_response = wrap_request.send().promise.await.unwrap();
        let wrapped_client = wrap_response.get().unwrap().get_result().unwrap();

        let e = task::LocalSet::new()
            .run_until(async {
                let mut spawn_request = wrapped_client.spawn_request();
                let builder = spawn_request.get();
                let posix_args = builder.init_args();
                let args = posix_args.init_config();
                let mut config: hello_world::hello_world_capnp::config::Builder = args.init_as();
                config.set_greeting("Hello".into());

                // TODO: Pass in the hello_world structural config parameters

                let promise = spawn_request.send().promise;
                let response = promise.await?;
                let process_client = response.get()?.get_result()?;

                let api_response = process_client.get_api_request().send().promise.await?;
                let hello_client: hello_world::hello_world_capnp::root::Client =
                    api_response.get()?.get_api()?.get_as_capability()?;

                let mut sayhello = hello_client.say_hello_request();
                sayhello.get().init_request().set_name("Keystone".into());
                let hello_response = sayhello.send().promise.await?;

                let msg = hello_response.get()?.get_reply()?.get_message()?;

                assert_eq!(msg, "Hello, Keystone!");

                let geterror_response = process_client.get_error_request().send().promise.await?;
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
*/
