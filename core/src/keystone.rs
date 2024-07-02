use crate::cap_replacement::CapReplacement;
use crate::cell::SimpleCellImpl;
use crate::keystone_capnp::cap_expr;
use crate::proxy::ProxyServer;
use caplog::{CapLog, MAX_BUFFER_SIZE};
use capnp::any_pointer::Owned as any_pointer;
use capnp::capability::{FromClientHook, RemotePromise};
use capnp::private::capability::ClientHook;
use capnp_rpc::CapabilityServerSet;
use eyre::Result;
use std::time::Duration;
use std::{cell::RefCell, collections::HashMap, marker::PhantomData, path::Path, rc::Rc};

use crate::{
    cap_std_capnproto::AmbientAuthorityImpl,
    database::RootDatabase,
    keystone_capnp::{host, keystone_config},
    module_capnp::module_error,
    posix_module::PosixModuleImpl,
    posix_module_capnp::{posix_module, posix_module_args},
    spawn::posix_process::PosixProgramImpl,
};
type SpawnProgram = crate::spawn_capnp::program::Client<
    posix_module_args::Owned<any_pointer>,
    any_pointer,
    module_error::Owned<any_pointer>,
>;
type SpawnProcess =
    crate::spawn_capnp::process::Client<any_pointer, module_error::Owned<any_pointer>>;
type SpawnResults = crate::spawn_capnp::program::spawn_results::Owned<
    posix_module_args::Owned<any_pointer>,
    any_pointer,
    module_error::Owned<any_pointer>,
>;

use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unexpected end of file while trying to read section[{0},{1}]")]
    UnexpectedEof(usize, usize),
    #[error("Couldn't find any embedded schema data.")]
    NoSchemaData,
    #[error("File is not a valid compiled schema.")]
    NotValidSchema,
    #[error("A config file cannot provide a default value for an automatically resolved cell.")]
    CannotAssignAutocell,
    #[error("Module with ID {0} couldn't be found.")]
    ModuleNotFound(u64),
    #[error("Module with name {0} couldn't be found.")]
    ModuleNameNotFound(String),
    #[error("Couldn't find {0} field in {}")]
    MissingFieldTOML(String),
    #[error("Couldn't find {0} in {1}")]
    MissingSchemaField(String, String),
    #[error("TOML value {0} was not a {1}!")]
    InvalidTypeTOML(String, String),
    #[error("Capnproto value {0} was not a {1}!")]
    InvalidTypeCapstone(String, String),
    #[error("{0} is not a valid enumeration value!")]
    InvalidEnumValue(String),
    #[error("Capnproto type {0} is not a {1}!")]
    TypeMismatchCapstone(String, String),
    #[error("{0} is of type {1} which is not supported in this context.")]
    UnsupportedType(String, String),
    #[error("Deferred {0} still doesn't exist?!")]
    DeferredNotFound(String),
}

pub struct HostImpl<State> {
    instance_id: u64,
    phantom: PhantomData<State>,
}

impl<State> HostImpl<State>
where
    State: ::capnp::traits::Owned,
{
    pub fn new(id: u64) -> Self {
        Self {
            instance_id: id,
            phantom: PhantomData,
        }
    }
}

impl host::Server<capnp::any_pointer::Owned> for HostImpl<capnp::any_pointer::Owned> {}

#[derive(Debug, Serialize, Deserialize)]
pub enum ModuleState {
    NotStarted,
    Initialized, // Started but waiting for bootstrap capability to return
    Ready,
    Paused,
    Closing, // Has been told to shut down but is saving it's state
    Closed,  // Clean shutdown, can be restarted safely
    Aborted, // Was abnormally terminated for some reason
    StartFailure,
    CloseFailure,
}

// This can't be a rust generic because we do not know the type parameters at compile time.
pub struct ModuleInstance {
    instance_id: u64,
    name: String,
    client: Option<SpawnProgram>,
    process: Option<SpawnProcess>,
    pub api: Option<
        RemotePromise<
            crate::spawn_capnp::process::get_api_results::Owned<
                any_pointer,
                module_error::Owned<any_pointer>,
            >,
        >,
    >,
    state: ModuleState,
    queue: capnp_rpc::queued::Client,
}

pub struct Keystone {
    db: Rc<RefCell<RootDatabase>>,
    log: CapLog<MAX_BUFFER_SIZE>,
    file_server: Rc<RefCell<AmbientAuthorityImpl>>,
    pub modules: HashMap<u64, ModuleInstance>,
    pub namemap: HashMap<String, u64>,
    pub cells: Rc<
        RefCell<
            CapabilityServerSet<SimpleCellImpl, crate::storage_capnp::cell::Client<any_pointer>>,
        >,
    >,
    timeout: Duration,
    proxyset: Rc<RefCell<CapabilityServerSet<ProxyServer, capnp::capability::Client>>>, // Set of all untyped proxies
}

impl Keystone {
    pub async fn new_from_string(source: &str) -> Result<Self> {
        let mut message = ::capnp::message::Builder::new_default();
        let mut msg = message.init_root::<keystone_config::Builder>();
        crate::config::to_capnp(&source.parse::<toml::Table>()?, msg.reborrow())?;

        let mut instance = Keystone::new(
            message.get_root_as_reader::<keystone_config::Reader>()?,
            false,
        )?;
        instance
            .init(
                message
                    .get_root_as_reader::<keystone_config::Reader>()
                    .unwrap(),
            )
            .await?;
        Ok(instance)
    }

    pub fn new(config: keystone_config::Reader, check_consistency: bool) -> Result<Self> {
        let caplog_config = config.get_caplog()?;
        let mut db: RootDatabase = crate::database::Manager::open_database(
            Path::new(config.get_database()?.to_str()?),
            crate::database::OpenOptions::Create,
        )?;
        let modules = config.get_modules().map_or(HashMap::new(), |modules| {
            modules
                .iter()
                .map(|s| -> (u64, ModuleInstance) {
                    let name = s.get_name().unwrap().to_str().unwrap();
                    let id = db.get_string_index(name).unwrap() as u64;
                    (
                        id,
                        ModuleInstance {
                            name: name.to_owned(),
                            instance_id: id,
                            client: None,
                            process: None,
                            api: None,
                            state: ModuleState::NotStarted,
                            queue: capnp_rpc::queued::Client::new(None),
                        },
                    )
                })
                .collect()
        });

        let namemap = modules
            .iter()
            .map(|(id, m)| (m.name.clone(), *id))
            .collect();

        Ok(Self {
            db: Rc::new(RefCell::new(db)),
            log: CapLog::<MAX_BUFFER_SIZE>::new(
                caplog_config.get_max_file_size(),
                Path::new(caplog_config.get_trie_file()?.to_str()?),
                Path::new(caplog_config.get_data_prefix()?.to_str()?),
                caplog_config.get_max_open_files() as usize,
                check_consistency,
            )?,
            file_server: Rc::new(RefCell::new(AmbientAuthorityImpl::new())),
            modules,
            timeout: Duration::from_millis(config.get_ms_timeout()),
            cells: Rc::new(RefCell::new(CapabilityServerSet::new())),
            proxyset: Rc::new(RefCell::new(crate::proxy::CapSet::new())),
            namemap,
        })
    }

    async fn wrap_posix(
        client: crate::spawn::posix_process::PosixProgramClient,
    ) -> Result<SpawnProgram> {
        let wrapper_server = PosixModuleImpl {};
        let wrapper_client: posix_module::Client = capnp_rpc::new_client(wrapper_server);

        let mut wrap_request = wrapper_client.wrap_request();
        wrap_request.get().set_prog(client);
        let wrap_response = wrap_request.send().promise.await?;
        Ok(wrap_response.get()?.get_result()?)
    }

    async fn posix_spawn(
        config: keystone_config::module_config::Reader<'_, any_pointer>,
    ) -> Result<SpawnProgram> {
        let spawn_process_server =
            PosixProgramImpl::new_std(std::fs::File::open(config.get_path()?.to_str()?).unwrap());

        let spawn_process_client: crate::spawn::posix_process::PosixProgramClient =
            capnp_rpc::new_client(spawn_process_server);

        Self::wrap_posix(spawn_process_client).await
    }

    #[inline]
    fn extract_config_pair(
        config: keystone_config::module_config::Reader<'_, any_pointer>,
    ) -> Result<(capnp::any_pointer::Reader, &Path), capnp::Error> {
        Ok((
            config.get_config()?,
            Path::new(config.get_path()?.to_str()?),
        ))
    }

    fn process_spawn_request(
        x: Result<capnp::capability::Response<SpawnResults>, capnp::Error>,
    ) -> Result<SpawnProcess, capnp::Error> {
        x?.get()?.get_result()
    }

    fn recurse_cap_expr(
        &self,
        reader: cap_expr::Reader<'_>,
        cap_table: capnp::struct_list::Reader<'_, cap_expr::Owned>,
    ) -> capnp::Result<capnp::any_pointer::Pipeline> {
        Ok(match reader.which()? {
            cap_expr::Which::ModuleRef(r) => {
                let k = r?.to_string()?;
                let id = self
                    .namemap
                    .get(&k)
                    .ok_or(capnp::Error::failed("couldn't find module!".into()))?;
                capnp::any_pointer::Pipeline::new(Box::new(capnp_rpc::rpc::SingleCapPipeline::new(
                    self.proxyset
                        .borrow_mut()
                        .new_client(ProxyServer::new(
                            self.modules[id].queue.add_ref(),
                            self.proxyset.clone(),
                        ))
                        .hook,
                )))
            }
            cap_expr::Which::Field(r) => {
                let base = self.recurse_cap_expr(r.get_base()?, cap_table)?;
                base.get_pointer_field(r.get_index())
            }
            cap_expr::Which::Method(r) => {
                let base = self.recurse_cap_expr(r.get_subject()?, cap_table)?;
                let args = r.get_args();
                let mut call = base.as_cap().new_call(
                    r.get_interface_id(),
                    r.get_method_id(),
                    Some(args.target_size()?),
                );
                let replacement = CapReplacement::new(args, |index, builder| {
                    self.resolve_cap_expr(cap_table.get(index), cap_table, builder)
                });
                call.get().set_as(replacement)?;
                call.send().pipeline
            }
        })
    }

    fn resolve_cap_expr(
        &self,
        reader: cap_expr::Reader<'_>,
        cap_table: capnp::struct_list::Reader<'_, cap_expr::Owned>,
        mut builder: capnp::any_pointer::Builder<'_>,
    ) -> capnp::Result<()> {
        builder.set_as_capability(self.recurse_cap_expr(reader, cap_table)?.as_cap());
        Ok(())
    }

    fn failed_start(
        modules: &mut HashMap<u64, ModuleInstance>,
        id: u64,
        e: capnp::Error,
    ) -> Result<()> {
        let module = modules.get_mut(&id).ok_or(Error::ModuleNotFound(id))?;
        module.state = ModuleState::StartFailure;
        capnp_rpc::queued::ClientInner::resolve(&module.queue.inner, Err(e.clone()));
        Err(e.into())
    }

    async fn init_module(
        &mut self,
        id: u64,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
        cap_table: capnp::struct_list::Reader<'_, cap_expr::Owned>,
    ) -> Result<()> {
        self.modules
            .get_mut(&id)
            .ok_or(Error::ModuleNotFound(id))?
            .state = ModuleState::Initialized;
        let client = Self::posix_spawn(config).await.ok();

        let (conf, workpath) = match Self::extract_config_pair(config) {
            Ok((c, d)) => (c, d),
            Err(e) => {
                return Self::failed_start(&mut self.modules, id, e);
            }
        };

        // Then we get a path to our current app dir
        let workpath = workpath.parent().unwrap_or(workpath);
        let dir = match cap_std::fs::Dir::open_ambient_dir(
            workpath,
            self.file_server.as_ref().borrow().authority,
        ) {
            Ok(x) => x,
            Err(e) => {
                return Self::failed_start(&mut self.modules, id, e.into());
            }
        };

        let dirclient = AmbientAuthorityImpl::new_dir(&self.file_server, dir);

        // Pass our pair of arguments to the spawn request
        if let Some(client) = client {
            let mut spawn_request = client.spawn_request();
            let builder = spawn_request.get();
            let replacement = CapReplacement::new(conf, |index, mut builder| {
                if !cap_table.get(index).has_module_ref() {
                    let client = self
                        .cells
                        .borrow_mut()
                        // Very important to use ::init() here so it gets initialized to a default value
                        .new_client(
                            SimpleCellImpl::init(id as i64, self.db.clone())
                                .map_err(|e| capnp::Error::failed(e.to_string()))?,
                        );
                    builder.set_as_capability(client.into_client_hook());
                    Ok(())
                } else {
                    self.resolve_cap_expr(cap_table.get(index), cap_table, builder)
                }
            });

            let mut pair = builder.init_args();
            let mut anybuild: capnp::any_pointer::Builder = pair.reborrow().init_config();
            if let Err(e) = anybuild.set_as(replacement) {
                return Self::failed_start(&mut self.modules, id, e);
            }
            pair.set_workdir(dirclient);

            let response = spawn_request.send().promise.await;
            let process = match Self::process_spawn_request(response) {
                Ok(x) => x,
                Err(e) => {
                    return Self::failed_start(&mut self.modules, id, e);
                }
            };

            let module = self.modules.get_mut(&id).ok_or(Error::ModuleNotFound(id))?;
            let p = process.get_api_request().send();
            capnp_rpc::queued::ClientInner::resolve(
                &module.queue.inner,
                Ok(p.pipeline.get_api().as_cap()),
            );
            module.process = Some(process);
            module.client = Some(client);
            module.api = Some(p);
            module.state = ModuleState::Ready;
        } else {
            return Self::failed_start(
                &mut self.modules,
                id,
                capnp::Error::failed("Failed to acquire client!".to_string()),
            );
        }

        Ok(())
    }

    fn get_id(
        &self,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
    ) -> Result<u64> {
        Ok(self
            .db
            .borrow_mut()
            .get_string_index(config.get_name()?.to_str()?)? as u64)
    }

    fn check_error(
        result: Result<
            capnp::capability::Response<
                crate::spawn_capnp::process::get_error_results::Owned<
                    any_pointer,
                    module_error::Owned<any_pointer>,
                >,
            >,
            capnp::Error,
        >,
    ) -> Result<ModuleState> {
        let r = result?;
        let moderr: module_error::Reader<any_pointer> = r.get()?.get_result()?;
        Ok(match moderr.which()? {
            module_error::Which::Backing(e) => {
                let e: crate::posix_spawn_capnp::posix_error::Reader = e?.get_as()?;
                if e.get_error_code() != 0 {
                    ModuleState::CloseFailure
                } else {
                    ModuleState::Closed
                }
            }
            _ => ModuleState::CloseFailure,
        })
    }

    pub async fn init(&mut self, config: keystone_config::Reader<'_>) -> Result<()> {
        let modules = config.get_modules()?;
        for s in modules.iter() {
            let id = self.get_id(s)?;
            if let Err(e) = self.init_module(id, s, config.get_cap_table()?).await {
                eprintln!("Module Start Failure: {}", e.to_string());
                // TODO: log error
            }
        }

        Ok(())
    }

    async fn kill_module(module: &mut ModuleInstance) {
        if let Some(p) = module.process.as_ref() {
            let _ = p.kill_request().send().promise.await;
        }

        module.state = ModuleState::Aborted;
    }

    fn halted(state: &ModuleState) -> bool {
        match state {
            ModuleState::NotStarted => true,
            ModuleState::Closed => true,
            ModuleState::Aborted => true,
            ModuleState::StartFailure => true,
            ModuleState::CloseFailure => true,
            _ => false,
        }
    }

    pub async fn stop_module(module: &mut ModuleInstance, timeout: Duration) -> Result<()> {
        if Self::halted(&module.state) {
            return Ok(());
        }
        // TODO: If a race condition here is possible, this must be made atomic and checked to see if it was already set to closing by another thread.
        module.state = ModuleState::Closing;

        // Acquire the underlying process object
        let process = module.process.as_ref().and_then(|p| {
            crate::posix_module::PROCESS_SET.with_borrow(|x| x.get_local_server_of_resolved(p))
        });

        let stop_request = if let Some(p) = process {
            let borrow = p.as_ref().server.borrow_mut();
            borrow.bootstrap.stop_request().send()
        } else {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::Disconnected).into());
        };

        // Call the stop method with some timeout
        if (tokio::time::timeout(timeout, stop_request.promise).await).is_err() {
            // Force kill the module.
            Self::kill_module(module).await;
            Ok(())
        } else {
            if let Some(p) = module.process.as_ref() {
                // Now call the get error message with the same timeout
                match tokio::time::timeout(timeout, p.get_error_request().send().promise).await {
                    Ok(result) => {
                        module.state =
                            Self::check_error(result).unwrap_or(ModuleState::CloseFailure);
                    }
                    Err(_) => Self::kill_module(module).await,
                }
            }
            Ok(())
        }
    }

    pub async fn shutdown(&mut self) {
        for v in self.modules.values_mut() {
            // TODO: initiate all module closing attempts in parallel before awaiting
            let _ = match v.state {
                ModuleState::Closing => Ok(()),
                _ => Self::stop_module(v, self.timeout).await,
            };
        }
    }

    pub fn get_api_pipe<T: FromClientHook>(&self, module: &str) -> Result<T> {
        let id = self
            .namemap
            .get(module)
            .ok_or(Error::ModuleNameNotFound(module.into()))?;
        let module = self.modules.get(id).ok_or(Error::ModuleNotFound(*id))?;
        let pipe = module
            .api
            .as_ref()
            .ok_or(Error::DeferredNotFound("api ref".into()))?
            .pipeline
            .get_api()
            .as_cap();

        Ok(capnp::capability::FromClientHook::new(pipe))
    }
}

/*impl Drop for Keystone {
    fn drop(&mut self) {
        for (_, m) in self.modules.iter() {
            if !Self::halted(&m.state) {
                // There's a module that wasn't properly halted, but we can't start an additional runtime to handle it, so we panic
                panic!("Keystone instance dropped while {} was not halted!", m.name);
            }
        }
    }
}*/

#[cfg(test)]
use std::future::Future;
#[cfg(test)]
use tempfile::NamedTempFile;

#[cfg(test)]
pub fn test_harness<F: Future<Output = capnp::Result<()>> + 'static>(
    config: &str,
    f: impl FnOnce(Keystone) -> F + 'static,
) -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();

    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = keystone_util::build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(config);

    crate::config::to_capnp(&source.parse::<toml::Table>()?, msg.reborrow())?;

    let mut instance = Keystone::new(
        message.get_root_as_reader::<keystone_config::Reader>()?,
        false,
    )?;

    // TODO: might be able to replace the runtime catch below with .unhandled_panic(UnhandledPanic::ShutdownRuntime) if gets stabilized
    let pool = tokio::task::LocalSet::new();
    let fut = pool.run_until(async move {
        tokio::task::spawn_local(async move {
            instance
                .init(message.get_root_as_reader::<keystone_config::Reader>()?)
                .await
                .unwrap();

            f(instance).await?;
            Ok::<(), capnp::Error>(())
        })
        .await
    });

    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(fut);
    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    result.unwrap().unwrap();

    Ok(())
}

#[cfg(test)]
async fn test_hello_world_init_internal(mut instance: Keystone) -> Result<(), capnp::Error> {
    let hello_client: crate::hello_world_capnp::root::Client =
        instance.get_api_pipe("Hello World").unwrap();

    let mut sayhello = hello_client.say_hello_request();
    sayhello.get().init_request().set_name("Keystone".into());
    let hello_response = sayhello.send().promise.await?;

    let msg = hello_response.get()?.get_reply()?.get_message()?;

    assert_eq!(msg, "Bonjour, Keystone!");

    instance.shutdown().await;
    Ok(())
}

#[test]
fn test_hello_world_init() -> Result<()> {
    test_harness(
        &keystone_util::build_module_config(
            "Hello World",
            "hello-world-module",
            r#"{  greeting = "Bonjour" }"#,
        ),
        test_hello_world_init_internal,
    )
}

#[cfg(test)]
async fn test_stateful_internal(mut instance: Keystone) -> Result<(), capnp::Error> {
    let stateful_client: crate::stateful_capnp::root::Client =
        instance.get_api_pipe("Stateful").unwrap();

    {
        let mut echo = stateful_client.echo_last_request();
        echo.get().init_request().set_name("Keystone".into());
        let echo_response = echo.send().promise.await?;

        let msg = echo_response.get()?.get_reply()?.get_message()?;

        assert_eq!(msg, "echo ");
    }

    {
        let mut echo = stateful_client.echo_last_request();
        echo.get().init_request().set_name("Replace".into());
        let echo_response = echo.send().promise.await?;

        let msg = echo_response.get()?.get_reply()?.get_message()?;

        assert_eq!(msg, "echo Keystone");
    }

    {
        let mut echo = stateful_client.echo_last_request();
        echo.get().init_request().set_name("Replace".into());
        let echo_response = echo.send().promise.await?;

        let msg = echo_response.get()?.get_reply()?.get_message()?;

        assert_eq!(msg, "echo Replace");
    }

    instance.shutdown().await;
    Ok(())
}

#[test]
fn test_stateful() -> Result<()> {
    test_harness(
        &keystone_util::build_module_config(
            "Stateful",
            "stateful-module",
            r#"{ echoWord = "echo" }"#,
        ),
        test_stateful_internal,
    )
}

#[test]
fn test_hello_world_proxy() -> Result<()> {
    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = keystone_util::build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(&keystone_util::build_module_config(
        "Hello World",
        "hello-world-module",
        r#"{  greeting = "Bonjour" }"#,
    ));

    let pool = tokio::task::LocalSet::new();
    let fut = pool.run_until(async move {
        tokio::task::spawn_local(async move {
            let mut instance = Keystone::new_from_string(&source).await.unwrap();

            let module = &instance.modules[&instance.namemap["Hello World"]];
            let pipe = instance
                .proxyset
                .borrow_mut()
                .new_client(ProxyServer::new(
                    module.queue.add_ref(),
                    instance.proxyset.clone(),
                ))
                .hook;

            let hello_client: crate::hello_world_capnp::root::Client =
                capnp::capability::FromClientHook::new(pipe);

            let mut sayhello = hello_client.say_hello_request();
            sayhello.get().init_request().set_name("Keystone".into());
            let hello_response = sayhello.send().promise.await?;

            let msg = hello_response.get()?.get_reply()?.get_message()?;

            assert_eq!(msg, "Bonjour, Keystone!");

            instance.shutdown().await;
            Ok::<(), capnp::Error>(())
        })
        .await
    });

    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(fut);
    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    result.unwrap().unwrap();

    Ok(())
}
