use crate::cap_replacement::CapReplacement;
use crate::cell::SimpleCellImpl;
use crate::database::DatabaseExt;
pub use crate::proxy::ProxyServer;
use crate::util::SnowflakeSource;
use caplog::{CapLog, MAX_BUFFER_SIZE};
use capnp::any_pointer::Owned as any_pointer;
use capnp::capability::FromServer;
use capnp::capability::{FromClientHook, RemotePromise};
use capnp::private::capability::ClientHook;
use capnp::traits::SetPointerBuilder;
use capnp_rpc::twoparty::VatNetwork;
use capnp_rpc::{rpc_twoparty_capnp, CapabilityServerSet, RpcSystem};
use eyre::Result;
use eyre::WrapErr;
use futures_util::stream::FuturesUnordered;
use futures_util::FutureExt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use std::{cell::RefCell, collections::HashMap, path::Path, rc::Rc};

pub trait CapnpResult<T> {
    fn to_capnp(self) -> capnp::Result<T>;
}

impl<T, E: ToString> CapnpResult<T> for Result<T, E> {
    fn to_capnp(self) -> capnp::Result<T> {
        if self.is_err() {
            self.map_err(|e| capnp::Error::failed(e.to_string()))
        } else {
            self.map_err(|e| capnp::Error::failed(e.to_string()))
        }
    }
}

use super::{
    keystone_capnp::cap_expr,
    keystone_capnp::keystone_config,
    module_capnp::module_error,
    module_capnp::module_start,
    posix_module_capnp::{posix_module, posix_module_args},
};

use crate::{
    cap_std_capnproto::AmbientAuthorityImpl, host::HostImpl, posix_module::PosixModuleImpl,
    posix_process::PosixProgramImpl, scheduler::Scheduler, sqlite::SqliteDatabase,
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

pub type CellCapSet =
    CapabilityServerSet<SimpleCellImpl, crate::storage_capnp::cell::Client<any_pointer>>;

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
    #[error("Couldn't find {0} field")]
    MissingFieldTOML(String),
    #[error("Couldn't find {0} in {1}")]
    MissingSchemaField(String, String),
    #[error("Couldn't find schema for {0}")]
    MissingSchema(String),
    #[error("Couldn't find type for {0} with id {1}!")]
    MissingType(String, u64),
    #[error("Couldn't find {0} in {1}!")]
    MissingMethod(String, String),
    #[error("Method {0} did not specify a parameter list! If a method takes no parameters, you must provide an empty parameter list.")]
    MissingMethodParameters(String),
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
    #[error("Invalid config, {0}!")]
    InvalidConfig(String),
    #[error("Bootstrap interface for {0} is either missing or it was already closed!")]
    MissingBootstrap(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    module_id: u64,
    name: String,
    program: Option<SpawnProgram>,
    process: Option<SpawnProcess>,
    bootstrap: Option<module_start::Client<any_pointer, any_pointer>>,
    pub api: Option<
        RemotePromise<
            crate::spawn_capnp::process::get_api_results::Owned<
                any_pointer,
                module_error::Owned<any_pointer>,
            >,
        >,
    >,
    state: ModuleState,
    pub queue: capnp_rpc::queued::Client,
}

pub type ModuleJoinSet = Rc<RefCell<tokio::task::JoinSet<Result<()>>>>;
pub type RpcSystemSet = FuturesUnordered<Pin<Box<dyn Future<Output = Result<()>>>>>;
pub struct Keystone {
    db: Rc<crate::sqlite_capnp::root::ServerDispatch<SqliteDatabase>>,
    scheduler: Rc<crate::scheduler_capnp::root::ServerDispatch<Scheduler>>,
    log: Rc<RefCell<CapLog<MAX_BUFFER_SIZE>>>, // TODO: Remove RefCell once CapLog is made thread-safe.
    file_server: Rc<RefCell<AmbientAuthorityImpl>>,
    pub modules: HashMap<u64, ModuleInstance>,
    pub namemap: HashMap<String, u64>,
    pub cells: Rc<RefCell<CellCapSet>>,
    timeout: Duration,
    pub proxy_set: Rc<RefCell<crate::proxy::CapSet>>, // Set of all untyped proxies
    module_process_set: Rc<RefCell<crate::posix_module::ModuleProcessCapSet>>,
    process_set: Rc<RefCell<crate::posix_process::ProcessCapSet>>,
    pub rpc_systems: RpcSystemSet,
    snowflake: Rc<SnowflakeSource>,
}

pub const BUILTIN_KEYSTONE: &str = "keystone";
pub const BUILTIN_SQLITE: &str = "sqlite";
pub const BUILTIN_SCHEDULER: &str = "scheduler";
const BUILTIN_MODULES: [&str; 3] = [BUILTIN_KEYSTONE, BUILTIN_SQLITE, BUILTIN_SCHEDULER];

impl Keystone {
    pub async fn init_single_module<
        T: tokio::io::AsyncRead + 'static + Unpin,
        U: tokio::io::AsyncWrite + 'static + Unpin,
        API: FromClientHook,
    >(
        config: impl AsRef<str>,
        module: impl AsRef<str>,
        reader: T,
        writer: U,
    ) -> Result<(Self, API)> {
        let mut message = ::capnp::message::Builder::new_default();
        let mut msg = message.init_root::<keystone_config::Builder>();
        crate::config::to_capnp(
            &config.as_ref().parse::<toml::Table>()?,
            msg.reborrow(),
            &std::env::current_dir()?,
        )?;

        let mut instance = Keystone::new(
            message.get_root_as_reader::<keystone_config::Reader>()?,
            false,
        )?;

        let config = message.get_root_as_reader::<keystone_config::Reader>()?;
        let modules = config.get_modules()?;
        let mut target = None;
        for s in modules.iter() {
            let id = instance.get_id(s)?;
            if s.get_name()?.to_str()? == module.as_ref() {
                target = Some(s);
            } else {
                instance
                    .init_module(id, s, &std::env::current_dir()?, config.get_cap_table()?)
                    .await?;
            }
        }

        let span = tracing::debug_span!("Module", module = module.as_ref());
        let _enter = span.enter();

        let cap_table = config.get_cap_table()?;
        let (rpc_system, api, id, bootstrap) = if let Some(s) = target {
            tracing::debug!("Found target module, initializing in current thread");
            let id = instance.get_id(s)?;

            let (conf, _) = Self::extract_config_pair(s)?;
            let replacement = CapReplacement::new(conf, |index, mut builder| {
                if instance.autoinit_check(cap_table, builder.reborrow(), index as u32, id)? {
                    Ok(())
                } else {
                    instance.resolve_cap_expr(cap_table.get(index as u32), cap_table, builder)
                }
            });

            let host: crate::keystone_capnp::host::Client<any_pointer> =
                capnp_rpc::new_client(HostImpl::new(id, instance.db.clone()));

            tracing::debug!("Initializing RPC system");
            let (rpc_system, bootstrap, _, api) =
                init_rpc_system(reader, writer, host.client, replacement)?;

            Ok((rpc_system, api, id, bootstrap))
        } else {
            Err(Error::ModuleNameNotFound(module.as_ref().to_string()))
        }?;

        let module_name = module.as_ref().to_string();

        tracing::debug!("Resolving API proxy");
        let module = instance
            .modules
            .get_mut(&id)
            .ok_or(Error::ModuleNotFound(id))?;
        capnp_rpc::queued::ClientInner::resolve(
            &module.queue.inner,
            Ok(api.pipeline.get_api().as_cap()),
        );
        module.state = ModuleState::Ready;
        module.bootstrap = Some(bootstrap);

        let fut = async move { rpc_system.await.wrap_err(module_name) };

        instance.rpc_systems.push(fut.boxed_local());
        Ok((
            instance,
            capnp::capability::FromClientHook::new(api.pipeline.get_api().as_cap()),
        ))
    }

    pub fn new(config: keystone_config::Reader, check_consistency: bool) -> Result<Self> {
        let span = tracing::info_span!("Initialization");
        let _enter = span.enter();

        {
            let val: capnp::dynamic_value::Reader = config.into();
            tracing::debug!(config = ?val, check_consistency = check_consistency);
        }

        let caplog_config = config.get_caplog()?;
        let db = crate::database::open_database(
            Path::new(config.get_database()?.to_str()?),
            SqliteDatabase::new_connection,
            crate::database::OpenOptions::Create,
        )?;
        let scheduler = Rc::new(crate::scheduler_capnp::root::Client::from_server(
            Scheduler::new(db.clone(), ())?,
        ));

        let builtin =
            BUILTIN_MODULES.map(|s| (s.to_string(), db.get_string_index(s).unwrap() as u64));

        tracing::debug!("Iterating through {} modules", config.get_modules()?.len());
        let modules = config.get_modules().map_or(HashMap::new(), |modules| {
            modules
                .iter()
                .map(|s| -> (u64, ModuleInstance) {
                    let name = s.get_name().unwrap().to_str().unwrap();
                    let id = db.get_string_index(name).unwrap() as u64;
                    tracing::info!("Found {} module with id {}", name, id);
                    (
                        id,
                        ModuleInstance {
                            name: name.to_owned(),
                            module_id: id,
                            program: None,
                            process: None,
                            bootstrap: None,
                            api: None,
                            state: ModuleState::NotStarted,
                            queue: capnp_rpc::queued::Client::new(None),
                        },
                    )
                })
                .collect()
        });

        let namemap: HashMap<String, u64> = modules
            .iter()
            .map(|(id, m)| (m.name.clone(), *id))
            .chain(builtin)
            .collect();

        {
            let mut clients = db.server.clients.borrow_mut();
            clients.extend(
                modules
                    .iter()
                    .map(|(id, instance)| (*id, instance.queue.add_ref())),
            );

            clients.insert(
                namemap[BUILTIN_SQLITE],
                capnp_rpc::local::Client::from_rc(db.clone()).add_ref(),
            );

            clients.insert(
                namemap[BUILTIN_SCHEDULER],
                capnp_rpc::local::Client::from_rc(scheduler.clone()).add_ref(),
            );
        }

        #[cfg(miri)]
        let caplog = CapLog::<MAX_BUFFER_SIZE>::new_storage(
            caplog_config.get_max_file_size(),
            caplog::hashed_array_trie::Storage::new(std::path::Path::new("/"), 2_u64.pow(8))?,
            Path::new(caplog_config.get_data_prefix()?.to_str()?),
            caplog_config.get_max_open_files() as usize,
            check_consistency,
        )?;

        #[cfg(not(miri))]
        let caplog = CapLog::<MAX_BUFFER_SIZE>::new(
            caplog_config.get_max_file_size(),
            Path::new(caplog_config.get_trie_file()?.to_str()?),
            Path::new(caplog_config.get_data_prefix()?.to_str()?),
            caplog_config.get_max_open_files() as usize,
            check_consistency,
        )?;

        Ok(Self {
            db,
            scheduler,
            log: Rc::new(RefCell::new(caplog)),
            file_server: Default::default(),
            modules,
            timeout: Duration::from_millis(config.get_ms_timeout()),
            cells: Rc::new(RefCell::new(CapabilityServerSet::new())),
            proxy_set: Rc::new(RefCell::new(crate::proxy::CapSet::new())),
            module_process_set: Rc::new(RefCell::new(
                crate::posix_module::ModuleProcessCapSet::new(),
            )),
            process_set: Rc::new(RefCell::new(crate::posix_process::ProcessCapSet::new())),
            namemap,
            rpc_systems: Default::default(),
            snowflake: Rc::new(SnowflakeSource::new()),
        })
    }

    pub fn get_module_name(&self, id: u64) -> Result<&str, Error> {
        Ok(&self.modules.get(&id).ok_or(Error::ModuleNotFound(id))?.name)
    }
    pub fn log_capnp_params(params: &capnp::dynamic_value::Reader<'_>) {
        tracing::event!(tracing::Level::TRACE, parameters = format!("{:?}", params));
    }
    async fn wrap_posix(
        &self,
        host: crate::keystone_capnp::host::Client<any_pointer>,
        client: crate::posix_process::PosixProgramClient,
    ) -> Result<SpawnProgram> {
        let wrapper_server = PosixModuleImpl {
            host,
            module_process_set: self.module_process_set.clone(),
            process_set: self.process_set.clone(),
        };
        let wrapper_client: posix_module::Client = capnp_rpc::new_client(wrapper_server);

        let mut wrap_request = wrapper_client.wrap_request();
        wrap_request.get().set_prog(client);
        let wrap_response = wrap_request.send().promise.await?;
        Ok(wrap_response.get()?.get_result()?)
    }

    async fn posix_spawn(
        &self,
        host: crate::keystone_capnp::host::Client<any_pointer>,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
        dir: &Path,
    ) -> Result<SpawnProgram> {
        let path = dir.join(Path::new(config.get_path()?.to_str()?));
        let spawn_process_server =
            PosixProgramImpl::new_std(std::fs::File::open(&path)?, self.process_set.clone());

        let spawn_process_client: crate::posix_process::PosixProgramClient =
            capnp_rpc::new_client(spawn_process_server);

        self.wrap_posix(host, spawn_process_client).await
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

    fn internal_cap(&self, id: &str) -> Option<Box<dyn ClientHook>> {
        match id {
            BUILTIN_KEYSTONE => Some(
                capnp_rpc::new_client::<crate::keystone_capnp::root::Client, KeystoneRoot>(
                    KeystoneRoot::new(self),
                )
                .client
                .hook,
            ),
            BUILTIN_SQLITE => Some(capnp_rpc::local::Client::from_rc(self.db.clone()).add_ref()),
            BUILTIN_SCHEDULER => {
                Some(capnp_rpc::local::Client::from_rc(self.scheduler.clone()).add_ref())
            }
            _ => None,
        }
    }

    fn recurse_cap_expr(
        &self,
        reader: cap_expr::Reader<'_>,
        cap_table: capnp::struct_list::Reader<'_, cap_expr::Owned>,
    ) -> capnp::Result<capnp::any_pointer::Pipeline> {
        Ok(match reader.which()? {
            cap_expr::Which::ModuleRef(r) => {
                let k = r?.to_string()?;
                capnp::any_pointer::Pipeline::new(Box::new(capnp_rpc::rpc::SingleCapPipeline::new(
                    if let Some(hook) = self.internal_cap(&k) {
                        hook
                    } else {
                        let id = self
                            .namemap
                            .get(&k)
                            .ok_or(capnp::Error::failed("couldn't find module!".into()))?;
                        self.proxy_set
                            .borrow_mut()
                            .new_client(ProxyServer::new(
                                self.modules[id].queue.add_ref(),
                                self.proxy_set.clone(),
                                self.log.clone(),
                                self.snowflake.clone(),
                            ))
                            .hook
                    },
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
                    self.resolve_cap_expr(cap_table.get(index as u32), cap_table, builder)
                });
                call.get().set_as(replacement)?;
                call.send().pipeline
            }
        })
    }

    pub fn get_proxy_set(&self) -> Rc<RefCell<crate::proxy::CapSet>> {
        self.proxy_set.clone()
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
        tracing::error!(
            "Module {} failed to start with error {}",
            module.name,
            e.to_string()
        );
        Err(e.into())
    }

    fn autoinit_check(
        &self,
        cap_table: capnp::struct_list::Reader<'_, cap_expr::Owned>,
        mut builder: capnp::any_pointer::Builder<'_>,
        index: u32,
        id: u64,
    ) -> capnp::Result<bool> {
        if let crate::keystone_capnp::cap_expr::Which::ModuleRef(_) =
            cap_table.get(index).which()?
        {
            if !cap_table.get(index).has_module_ref() {
                let client = self
                    .cells
                    .borrow_mut()
                    // Very important to use ::init() here so it gets initialized to a default value
                    .new_client(SimpleCellImpl::init(id as i64, self.db.clone()).to_capnp()?);
                builder.set_as_capability(client.into_client_hook());
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn init_module(
        &mut self,
        id: u64,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
        config_dir: &Path,
        cap_table: capnp::struct_list::Reader<'_, cap_expr::Owned>,
    ) -> Result<()> {
        self.modules
            .get_mut(&id)
            .ok_or(Error::ModuleNotFound(id))?
            .state = ModuleState::Initialized;

        let span = tracing::debug_span!("Initializing", name = self.modules.get(&id).unwrap().name);
        let _enter = span.enter();
        tracing::debug!("Set Initialized flag");

        let keystone_host: crate::keystone_capnp::host::Client<any_pointer> =
            capnp_rpc::new_client(HostImpl::new(id, self.db.clone()));
        let Ok(program) = self
            .posix_spawn(keystone_host.clone(), config, config_dir)
            .await
        else {
            return Self::failed_start(
                &mut self.modules,
                id,
                capnp::Error::failed("Failed to spawn program!".to_string()),
            );
        };

        let (conf, workpath) = match Self::extract_config_pair(config) {
            Ok((c, d)) => (c, d),
            Err(e) => {
                return Self::failed_start(&mut self.modules, id, e);
            }
        };

        // Then we get a path to our current app dir
        let workpath = config_dir.join(workpath.parent().unwrap_or(workpath));
        let dir = match cap_std::fs::Dir::open_ambient_dir(
            &workpath,
            self.file_server.as_ref().borrow().authority,
        ) {
            Ok(x) => x,
            Err(e) => {
                return Self::failed_start(&mut self.modules, id, e.into());
            }
        };

        let dirclient = AmbientAuthorityImpl::new_dir(&self.file_server, dir);

        // Pass our pair of arguments to the spawn request
        let mut spawn_request = program.spawn_request();
        let builder = spawn_request.get();
        let replacement = CapReplacement::new(conf, |index, mut builder| {
            if self.autoinit_check(cap_table, builder.reborrow(), index as u32, id)? {
                Ok(())
            } else {
                self.resolve_cap_expr(cap_table.get(index as u32), cap_table, builder)
            }
        });

        let mut pair = builder.init_args();
        let mut anybuild: capnp::any_pointer::Builder = pair.reborrow().init_config();
        if let Err(e) = anybuild.set_as(replacement) {
            return Self::failed_start(&mut self.modules, id, e);
        }
        pair.set_workdir(dirclient);

        tracing::debug!("Sending spawn request inside {}", workpath.display());
        let response = spawn_request.send().promise.await;
        let process = match Self::process_spawn_request(response) {
            Ok(x) => x,
            Err(e) => {
                return Self::failed_start(&mut self.modules, id, e);
            }
        };

        let inner = self
            .module_process_set
            .borrow()
            .get_local_server_of_resolved(&process)
            .expect("Could not get inner process dispatch?");

        self.rpc_systems.push(
            inner
                .server
                .borrow_mut()
                .rpc_future
                .take()
                .expect("Lost rpc_future???"),
        );

        tracing::debug!("Sending API request");
        let module = self.modules.get_mut(&id).ok_or(Error::ModuleNotFound(id))?;
        let p = process.get_api_request().send();
        capnp_rpc::queued::ClientInner::resolve(
            &module.queue.inner,
            Ok(p.pipeline.get_api().as_cap()),
        );

        inner.server.borrow_mut().debug_name = Some(module.name.clone());

        module.process = Some(process);
        module.program = Some(program);
        module.bootstrap = inner.server.borrow_mut().bootstrap.take();
        module.api = Some(p);
        module.state = ModuleState::Ready;

        Ok(())
    }

    fn get_id(
        &self,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
    ) -> Result<u64> {
        Ok(self
            .db
            .server
            .get_string_index(config.get_name()?.to_str()?)? as u64)
    }

    fn check_error(
        name: &str,
        result: Result<
            capnp::capability::Response<
                crate::spawn_capnp::process::join_results::Owned<
                    any_pointer,
                    module_error::Owned<any_pointer>,
                >,
            >,
            capnp::Error,
        >,
    ) -> Result<ModuleState> {
        let r = match result {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{} process returned error: {}", name, e.to_string());
                return Err(e.into());
            }
        };

        let moderr: module_error::Reader<any_pointer> = r.get()?.get_result()?;
        Ok(match moderr.which()? {
            module_error::Which::Backing(e) => {
                let e: crate::posix_spawn_capnp::posix_error::Reader = e?.get_as()?;
                if e.get_error_code() != 0 {
                    tracing::error!(
                        "{} process returned error code: {}",
                        name,
                        e.get_error_code()
                    );
                    ModuleState::CloseFailure
                } else {
                    ModuleState::Closed
                }
            }
            _ => ModuleState::CloseFailure,
        })
    }

    pub async fn init(&mut self, dir: &Path, config: keystone_config::Reader<'_>) -> Result<()> {
        let modules = config.get_modules()?;
        for s in modules.iter() {
            let id = self.get_id(s)?;
            if let Err(e) = self.init_module(id, s, dir, config.get_cap_table()?).await {
                tracing::error!("Module Start Failure: {}", e);
            }
        }

        Ok(())
    }

    /*pub fn run<'a>(
        &'a mut self,
    ) -> (
        impl std::future::Future<Output = Result<(), Vec<eyre::ErrReport>>> + 'a,
        CancellationToken,
    ) {
        let cancel = CancellationToken::new();
        let cloned = cancel.clone();

        let fut = async move {
            let mut output: Vec<eyre::ErrReport> = Vec::new();
            tracing::info!("Keystone running");
            while !self.rpc_systems.is_empty() {
                let res = tokio::select! {
                    r = self.rpc_systems.next() => r,
                    _ = cloned.cancelled() => break,
                };
                match res {
                    Some(Ok(())) => (),
                    Some(Err((e, name))) => {
                        tracing::error!(
                            "{} RPC error: {}",
                            name.unwrap_or("[Unknown Module]".to_string()),
                            e
                        );
                        output.push(e.into());
                    }
                    None => (),
                };
            }

            if output.is_empty() {
                Ok(())
            } else {
                Err(output)
            }
        };

        (fut, cancel)
    }*/

    async fn kill_module(module: &mut ModuleInstance) {
        if let Some(p) = module.process.as_ref() {
            let _ = p.kill_request().send().promise.await;
        }

        module.state = ModuleState::Aborted;
    }

    fn halted(state: &ModuleState) -> bool {
        matches!(
            state,
            ModuleState::NotStarted
                | ModuleState::Closed
                | ModuleState::Aborted
                | ModuleState::StartFailure
                | ModuleState::CloseFailure
        )
    }

    pub async fn stop_module(module: &mut ModuleInstance, timeout: Duration) -> Result<()> {
        if Self::halted(&module.state) {
            return Ok(());
        }
        module.state = ModuleState::Closing;

        // Send the stop request to the bootstrap interface
        let Some(bootstrap) = module.bootstrap.as_ref() else {
            return Err(Error::MissingBootstrap(module.name.clone()).into());
        };

        let stop_request = bootstrap.stop_request().send();

        // Call the stop method with some timeout
        if (tokio::time::timeout(timeout, stop_request.promise).await).is_err() {
            // Force kill the module.
            Self::kill_module(module).await;
            Ok(())
        } else {
            if let Some(p) = module.process.as_ref() {
                // Now join the process with the same timeout
                match tokio::time::timeout(timeout, p.join_request().send().promise).await {
                    Ok(result) => {
                        module.state = match Self::check_error(&module.name, result) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::error!(
                                    "Failure during {} error lookup: {}",
                                    &module.name,
                                    e.to_string()
                                );
                                ModuleState::CloseFailure
                            }
                        };
                    }
                    Err(_) => Self::kill_module(module).await,
                }
            }
            Ok(())
        }
    }

    pub fn shutdown(
        &mut self,
    ) -> (
        FuturesUnordered<impl Future<Output = Result<()>> + use<'_>>,
        &mut RpcSystemSet,
    ) {
        // The scheduler thread is always running in an endless loop, so we abort it here.
        self.scheduler.server.thread.abort();

        let set: FuturesUnordered<_> = self
            .modules
            .values_mut()
            .filter_map(|v| {
                if v.state != ModuleState::Closing {
                    Some(Self::stop_module(v, self.timeout))
                } else {
                    None
                }
            })
            .collect();

        (set, &mut self.rpc_systems)
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

/* sadly we can't actually do this, because if an error happens the rpc_system won't be drained.
impl Drop for Keystone {
    fn drop(&mut self) {
        if !self.rpc_systems.is_empty() {
            panic!("Keystone instance dropped without waiting for proper shutdown!", m.name);
        }
    }
}*/

#[inline]
pub fn build_module_config(name: &str, binary: &str, config: &str) -> String {
    format!(
        r#"
    [[modules]]
    name = "{name}"
    path = "{}"
    config = {config}"#,
        get_binary_path(binary)
            .as_os_str()
            .to_str()
            .unwrap()
            .replace('\\', "/"),
    )
}

pub fn get_binary_path(name: &str) -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("couldn't get current EXE path");
    let mut target_dir = exe.parent().unwrap();

    if target_dir.ends_with("deps") {
        target_dir = target_dir.parent().unwrap();
    }

    #[cfg(windows)]
    {
        return target_dir.join(format!("{}.exe", name).as_str());
    }

    #[cfg(not(windows))]
    {
        return target_dir.join(name);
    }
}

pub struct KeystoneRoot {
    db: Rc<crate::sqlite_capnp::root::ServerDispatch<SqliteDatabase>>,
    cells: Rc<RefCell<CellCapSet>>,
}

impl KeystoneRoot {
    pub fn new(from: &Keystone) -> Self {
        Self {
            db: from.db.clone(),
            cells: from.cells.clone(),
        }
    }
}

impl crate::keystone_capnp::root::Server for KeystoneRoot {
    async fn init_cell(
        &self,
        params: crate::keystone_capnp::root::InitCellParams,
        mut results: crate::keystone_capnp::root::InitCellResults,
    ) -> Result<(), ::capnp::Error> {
        let span = tracing::debug_span!("host", id = BUILTIN_KEYSTONE);
        let _enter = span.enter();
        let params = params.get()?;
        tracing::debug!("init_cell()");
        let id = self
            .db
            .server
            .get_string_index(params.get_id()?.to_str()?)
            .to_capnp()?;

        let client = self
            .cells
            .borrow_mut()
            // Very important to use ::init() here so it gets initialized to a default value
            .new_client(SimpleCellImpl::init(id, self.db.clone()).to_capnp()?);

        if params.has_default() {
            self.db
                .server
                .set_state(id, params.get_default()?)
                .await
                .to_capnp()?;
        }

        results.get().set_result(client);
        Ok(())
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn init_rpc_system<
    T: tokio::io::AsyncRead + 'static + Unpin,
    U: tokio::io::AsyncWrite + 'static + Unpin,
    From: SetPointerBuilder,
>(
    reader: T,
    writer: U,
    bootstrap: capnp::capability::Client,
    config: From,
) -> capnp::Result<(
    RpcSystem<rpc_twoparty_capnp::Side>,
    module_start::Client<any_pointer, any_pointer>,
    capnp_rpc::Disconnector<rpc_twoparty_capnp::Side>,
    RemotePromise<module_start::start_results::Owned<any_pointer, any_pointer>>,
)> {
    let network = VatNetwork::new(
        reader,
        writer,
        rpc_twoparty_capnp::Side::Client,
        Default::default(),
    );

    let mut rpc_system = RpcSystem::new(Box::new(network), Some(bootstrap));

    let disconnector = rpc_system.get_disconnector();
    let bootstrap: module_start::Client<any_pointer, any_pointer> =
        rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);

    let mut api_request = bootstrap.start_request();
    api_request.get().init_config().set_as(config)?;

    let api = api_request.send();

    Ok((rpc_system, bootstrap, disconnector, api))
}

/// This extends the capability server set so it can store an AnyPointer version of a client, but retrieve it as a specialized client type
pub(crate) trait CapabilityServerSetExt<S, C>
where
    C: capnp::capability::FromServer<S>,
{
    fn new_generic_client<T: FromClientHook>(&mut self, s: S) -> T;
}

impl<S, C> CapabilityServerSetExt<S, C> for CapabilityServerSet<S, C>
where
    C: capnp::capability::FromServer<S>,
{
    fn new_generic_client<T: FromClientHook>(&mut self, s: S) -> T {
        FromClientHook::new(self.new_client(s).into_client_hook())
    }
}
