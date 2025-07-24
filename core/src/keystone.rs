use crate::cap_replacement::CapReplacement;
use crate::capnp::any_pointer::Owned as any_pointer;
use crate::capnp::capability::FromServer;
use crate::capnp::capability::{FromClientHook, RemotePromise};
use crate::capnp::private::capability::ClientHook;
use crate::capnp::traits::SetPointerBuilder;
use crate::capnp_rpc::twoparty::VatNetwork;
use crate::capnp_rpc::{self, CapabilityServerSet, RpcSystem, rpc_twoparty_capnp};
use crate::cell::SimpleCellImpl;
use crate::database::DatabaseExt;
use crate::module::*;
use crate::process::IdentifierImpl;
pub use crate::proxy::ProxyServer;
use crate::util::SnowflakeSource;
use crate::{cap_std_capnp, capnp};
use caplog::{CapLog, MAX_BUFFER_SIZE};
use eyre::Result;
use eyre::WrapErr;
use futures_util::FutureExt;
use futures_util::stream::FuturesUnordered;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use std::{cell::RefCell, collections::HashMap, path::Path, rc::Rc};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
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
    module_capnp::{module_args, module_error, module_start},
};

use crate::{
    cap_std_capnproto::AmbientAuthorityImpl, host::HostImpl, posix::PosixProgramImpl,
    scheduler::Scheduler, sqlite::SqliteDatabase,
};
pub type ModuleProgram = crate::spawn_capnp::program::Client<
    module_args::Owned<any_pointer, cap_std_capnp::dir::Owned>,
    any_pointer,
    module_error::Owned<any_pointer>,
>;
pub type ModuleProcess =
    crate::spawn_capnp::process::Client<any_pointer, module_error::Owned<any_pointer>>;
pub type SpawnResults = crate::spawn_capnp::program::spawn_results::Owned<
    module_args::Owned<any_pointer, cap_std_capnp::dir::Owned>,
    any_pointer,
    module_error::Owned<any_pointer>,
>;
pub type ModulePair = (
    module_start::Client<any_pointer, any_pointer>,
    Pin<Box<dyn Future<Output = eyre::Result<()>>>>,
);

pub type CellCapSet =
    CapabilityServerSet<SimpleCellImpl, crate::storage_capnp::cell::Client<any_pointer>>;

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
    #[error(
        "Method {0} did not specify a parameter list! If a method takes no parameters, you must provide an empty parameter list."
    )]
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

enum FutureStaging {
    None,
    Staged(Pin<Box<dyn Future<Output = Result<()>>>>),
    Running(JoinHandle<Result<()>>),
}

pub type ModuleJoinSet = Rc<RefCell<tokio::task::JoinSet<Result<()>>>>;
pub type RpcSystemSet = FuturesUnordered<Pin<Box<dyn Future<Output = Result<()>>>>>;

pub struct KeystoneRoot {
    pub(crate) db: Rc<SqliteDatabase>,
    pub(crate) cells: Rc<RefCell<CellCapSet>>,
    pub(crate) file_server: Rc<RefCell<AmbientAuthorityImpl>>,
    pub(crate) program_set: Rc<RefCell<crate::posix::ProgramCapSet>>,
    pub(crate) id_set: Rc<RefCell<crate::process::IdCapSet>>,
    pub(crate) log_tx: Option<UnboundedSender<(u64, String)>>,
}

pub struct Keystone {
    root: Rc<KeystoneRoot>,
    scheduler: Rc<Scheduler>,
    scheduler_thread: FutureStaging,
    pub log: Rc<RefCell<CapLog<MAX_BUFFER_SIZE>>>, // TODO: Remove RefCell once CapLog is made thread-safe.
    pub modules: HashMap<std::path::PathBuf, ModuleSource>,
    pub instances: HashMap<u64, ModuleInstance>,
    pub cap_functions: Vec<FunctionDescription>,
    pub namemap: HashMap<String, u64>,
    pub timeout: Duration,
    pub proxy_set: Rc<RefCell<crate::proxy::CapSet>>, // Set of all untyped proxies
    pub snowflake: Rc<SnowflakeSource>,
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
        log_tx: Option<UnboundedSender<(u64, String)>>,
    ) -> Result<(Self, API, RpcSystemSet)> {
        let mut message = capnp::message::Builder::new_default();
        let mut msg = message.init_root::<keystone_config::Builder>();
        crate::config::to_capnp(
            &config.as_ref().parse::<toml::Table>()?,
            msg.reborrow(),
            &std::env::current_dir()?,
        )?;

        let (mut instance, rpc_systems) = Self::new(
            message.get_root_as_reader::<keystone_config::Reader>()?,
            false,
            log_tx,
        )?;

        let config = message.get_root_as_reader::<keystone_config::Reader>()?;
        let modules = config.get_modules()?;
        let mut target = None;
        let default_log = Self::default_log_env(&config);
        for s in modules.iter() {
            if s.get_name()?.to_str()? == module.as_ref() {
                target = Some(s);
            } else {
                let (tx, rx) = oneshot::channel();
                let id = instance.gen_id(s, default_log.into(), tx)?;
                let instance_id = id.id(&instance.root.db)?;

                instance
                    .init_module(id, s, &std::env::current_dir()?, config.get_cap_table()?)
                    .await?;

                let (bootstrap, rpc_future) = rx.await?;

                let module = instance
                    .instances
                    .get_mut(&instance_id)
                    .expect("Module vanished!");

                module.bootstrap = Some(bootstrap);
                rpc_systems.push(rpc_future);
            }
        }

        let stage = match std::mem::replace(&mut instance.scheduler_thread, FutureStaging::None) {
            FutureStaging::None => FutureStaging::None,
            FutureStaging::Staged(pin) => FutureStaging::Running(tokio::task::spawn_local(pin)),
            FutureStaging::Running(join_handle) => FutureStaging::Running(join_handle),
        };

        instance.scheduler_thread = stage;

        let span = tracing::debug_span!("Module", module = module.as_ref());
        let _enter = span.enter();

        let cap_table = config.get_cap_table()?;
        let (rpc_system, api, instance_id, bootstrap) = if let Some(s) = target {
            tracing::debug!("Found target module, initializing in current thread");
            let instance_id =
                IdentifierImpl::get_id(&s.get_name()?.to_string()?, &instance.root.db)?;

            let (conf, _) = Self::extract_config_pair(s)?;
            let replacement = CapReplacement::new(conf, |index, mut builder| {
                if Keystone::autoinit_check(
                    cap_table,
                    builder.reborrow(),
                    index as u32,
                    instance_id,
                    instance.root.db.clone(),
                    instance.root.cells.clone(),
                )? {
                    Ok(())
                } else {
                    instance.resolve_cap_expr(cap_table.get(index as u32), cap_table, builder)
                }
            });

            let host: crate::keystone_capnp::host::Client<any_pointer> =
                capnp_rpc::new_client(HostImpl::new(instance_id, instance.root.db.clone()));

            tracing::debug!("Initializing RPC system");
            let (rpc_system, bootstrap, _, api) =
                init_rpc_system(reader, writer, host.client, replacement)?;

            Ok((rpc_system, api, instance_id, bootstrap))
        } else {
            Err(Error::ModuleNameNotFound(module.as_ref().to_string()))
        }?;

        let module_name = module.as_ref().to_string();

        tracing::debug!("Resolving API proxy");
        let module = instance
            .instances
            .get_mut(&instance_id)
            .ok_or(Error::ModuleNotFound(instance_id))?;
        capnp_rpc::queued::ClientInner::resolve(
            &module.api.inner,
            Ok(api.pipeline.get_api().as_cap()),
        );
        module.state.store(
            ModuleState::Ready as u8,
            std::sync::atomic::Ordering::Release,
        );
        module.bootstrap = Some(bootstrap);

        let fut = async move { rpc_system.await.wrap_err(module_name) };

        rpc_systems.push(fut.boxed_local());
        Ok((
            instance,
            capnp::capability::FromClientHook::new(api.pipeline.get_api().as_cap()),
            rpc_systems,
        ))
    }

    pub fn new(
        config: keystone_config::Reader,
        check_consistency: bool,
        log_tx: Option<UnboundedSender<(u64, String)>>,
    ) -> Result<(Self, RpcSystemSet)> {
        let span = tracing::info_span!("Initialization");
        let _enter = span.enter();

        {
            let val: capnp::dynamic_value::Reader = config.into();
            tracing::debug!(config = ?val, check_consistency = check_consistency);
        }

        let caplog_config = config.get_caplog()?;
        let db = crate::database::open_database(
            Path::new(config.get_database()?.to_str()?),
            |c| SqliteDatabase::new_connection(c).map(Rc::new),
            crate::database::OpenOptions::Create,
        )?;

        let (s, fut) = Scheduler::new(db.clone(), ())?;
        let scheduler = Rc::new(s);

        let builtin =
            BUILTIN_MODULES.map(|s| (s.to_string(), db.get_string_index(s).unwrap() as u64));
        let id_set = crate::process::IdCapSet::new();

        tracing::debug!("Iterating through {} modules", config.get_modules()?.len());
        let instances = config.get_modules().map_or(HashMap::new(), |modules| {
            modules
                .iter()
                .map(|s| -> (u64, ModuleInstance) {
                    let name = s.get_name().unwrap().to_str().unwrap();
                    let instance_id = IdentifierImpl::get_id(name, &db).unwrap();
                    tracing::info!("Found {name} module with id {instance_id}");
                    (
                        instance_id,
                        ModuleInstance {
                            name: name.into(),
                            process: None,
                            module_id: u64::MAX,
                            bootstrap: None.into(),
                            state: (ModuleState::NotStarted as u8).into(),
                            api: capnp_rpc::queued::Client::new(None),
                            pause: tokio::sync::watch::channel(false).0,
                        },
                    )
                })
                .collect()
        });

        let namemap: HashMap<String, u64> = instances
            .iter()
            .map(|(id, m)| (m.name.clone(), *id))
            .chain(builtin)
            .collect();

        {
            let mut clients = db.clients.borrow_mut();
            clients.extend(
                instances
                    .iter()
                    .map(|(id, instance)| (*id, instance.api.add_ref())),
            );

            clients.insert(
                namemap[BUILTIN_SQLITE],
                capnp_rpc::local::Client::new(crate::sqlite_capnp::root::Client::from_rc(
                    db.clone(),
                ))
                .add_ref(),
            );

            clients.insert(
                namemap[BUILTIN_SCHEDULER],
                capnp_rpc::local::Client::new(crate::scheduler_capnp::root::Client::from_rc(
                    scheduler.clone(),
                ))
                .add_ref(),
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
            true,
            check_consistency,
        )?;
        let cells = Rc::new(RefCell::new(CapabilityServerSet::new()));
        let root = Rc::new(KeystoneRoot {
            db,
            cells,
            file_server: Default::default(),
            program_set: Rc::new(RefCell::new(crate::posix::ProgramCapSet::new())),
            id_set: Rc::new(RefCell::new(id_set)),
            log_tx,
        });

        Ok((
            Self {
                scheduler,
                scheduler_thread: FutureStaging::Staged(fut.boxed_local()),
                root,
                log: Rc::new(RefCell::new(caplog)),
                instances,
                modules: Default::default(),
                timeout: Duration::from_millis(config.get_ms_timeout()),
                proxy_set: Rc::new(RefCell::new(crate::proxy::CapSet::new())),
                namemap,
                snowflake: Rc::new(SnowflakeSource::new()),
                cap_functions: Vec::new(),
            },
            Default::default(),
        ))
    }

    pub fn get_instance_name(&self, id: u64) -> Result<&str, Error> {
        Ok(&self
            .instances
            .get(&id)
            .ok_or(Error::ModuleNotFound(id))?
            .name)
    }
    pub fn log_capnp_params(params: &capnp::dynamic_value::Reader<'_>) {
        tracing::event!(tracing::Level::TRACE, parameters = format!("{:?}", params));
    }

    async fn parse_binary(
        &mut self,
        root: crate::posix_capnp::posix_module::Client,
        name: &str,
        instance_id: u64,
        path: &Path,
    ) -> Result<ModuleProgram> {
        if let Some(v) = self.modules.get(path) {
            Ok(v.program.clone())
        } else {
            let file_contents = std::fs::read(path)?;
            let binary = crate::binary_embed::load_deps_from_binary(&file_contents)?;
            let bufread = std::io::BufReader::new(binary);
            let dyn_schema = capnp::schema::DynamicSchema::new(capnp::serialize::read_message(
                bufread,
                capnp::message::ReaderOptions {
                    traversal_limit_in_words: None,
                    nesting_limit: 128,
                },
            )?)?;
            let schema: capnp::schema::CapabilitySchema =
                match dyn_schema.get_type_by_scope(&["Root"], None).unwrap() {
                    capnp::introspect::TypeVariant::Capability(s) => s.to_owned().into(),
                    _ => todo!(),
                };
            let module_id = schema.get_proto().get_id();

            match schema.get_proto().which().unwrap() {
                capnp::schema_capnp::node::Which::Interface(interface) => {
                    self.cap_functions.push(FunctionDescription {
                        module_or_cap: ModuleOrCap::ModuleId(instance_id),
                        function_name: name.into(),
                        type_id: 0,
                        method_id: 0,
                        params: Vec::new(),
                        params_schema: 0,
                        results: Vec::new(),
                        results_schema: 0,
                    });
                    fill_function_descriptions(
                        &mut self.cap_functions,
                        &dyn_schema,
                        interface,
                        module_id,
                        ModuleOrCap::ModuleId(instance_id),
                        &mut 0,
                    )?;
                }
                _ => todo!(),
            };

            let server = PosixProgramImpl::new_std(std::fs::File::open(&path)?);
            let client = self.root.program_set.borrow_mut().new_client(server);
            let wrap_response = root.build_wrap_request(client).send().promise.await?;

            self.modules.insert(
                path.to_path_buf(),
                ModuleSource {
                    module_id,
                    program: wrap_response.get()?.get_result()?,
                    dyn_schema,
                },
            );

            Ok(self.modules[path].program.clone())
        }
    }

    pub async fn load_module(
        &mut self,
        name: &str,
        instance_id: u64,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
        dir: &Path,
    ) -> Result<ModuleProgram> {
        let path = dir.join(Path::new(config.get_path()?.to_str()?));

        self.parse_binary(
            capnp_rpc::new_client_from_rc(self.root.clone()),
            name,
            instance_id,
            &path,
        )
        .await
    }

    #[inline]
    pub fn extract_config_pair(
        config: keystone_config::module_config::Reader<'_, any_pointer>,
    ) -> Result<(capnp::any_pointer::Reader, &Path), capnp::Error> {
        Ok((
            config.get_config()?,
            Path::new(config.get_path()?.to_str()?),
        ))
    }

    fn process_spawn_request(
        x: Result<capnp::capability::Response<SpawnResults>, capnp::Error>,
    ) -> Result<ModuleProcess, capnp::Error> {
        x?.get()?.get_result()
    }

    fn internal_cap(
        id: &str,
        db: Rc<SqliteDatabase>,
        root: Rc<KeystoneRoot>,
        scheduler: Rc<Scheduler>,
    ) -> Option<Box<dyn ClientHook>> {
        match id {
            BUILTIN_KEYSTONE => Some(Box::new(capnp_rpc::local::Client::new(
                crate::keystone_capnp::root::Client::from_rc(root.clone()),
            ))),
            BUILTIN_SQLITE => Some(Box::new(capnp_rpc::local::Client::new(
                crate::sqlite_capnp::root::Client::from_rc(db.clone()),
            ))),
            BUILTIN_SCHEDULER => Some(Box::new(capnp_rpc::local::Client::new(
                crate::scheduler_capnp::root::Client::from_rc(scheduler.clone()),
            ))),
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
                    if let Some(hook) = Self::internal_cap(
                        &k,
                        self.root.db.clone(),
                        self.root.clone(),
                        self.scheduler.clone(),
                    ) {
                        hook
                    } else {
                        let id = self
                            .namemap
                            .get(&k)
                            .ok_or(capnp::Error::failed("couldn't find module!".into()))?;
                        self.create_proxy(self.instances[id].api.add_ref()).hook
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

    pub fn autoinit_check(
        cap_table: capnp::struct_list::Reader<'_, cap_expr::Owned>,
        mut builder: capnp::any_pointer::Builder<'_>,
        index: u32,
        id: u64,
        db: Rc<SqliteDatabase>,
        cells: Rc<RefCell<CellCapSet>>,
    ) -> capnp::Result<bool> {
        if let crate::keystone_capnp::cap_expr::Which::ModuleRef(_) =
            cap_table.get(index).which()?
        {
            if !cap_table.get(index).has_module_ref() {
                let client = cells
                    .borrow_mut()
                    // Very important to use ::init() here so it gets initialized to a default value
                    .new_client(SimpleCellImpl::init(id as i64, db).to_capnp()?);
                builder.set_as_capability(client.into_client_hook());
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn failed_start(
        modules: &HashMap<u64, ModuleInstance>,
        id: u64,
        e: capnp::Error,
    ) -> Result<()> {
        let module = modules.get(&id).ok_or(Error::ModuleNotFound(id))?;
        module.state.store(
            ModuleState::StartFailure as u8,
            std::sync::atomic::Ordering::Release,
        );
        capnp_rpc::queued::ClientInner::resolve(&module.api.inner, Err(e.clone()));
        tracing::error!(
            "Module {} failed to start with error {}",
            module.name,
            e.to_string()
        );
        Err(e.into())
    }

    pub async fn init_module(
        &mut self,
        id: IdentifierImpl,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
        config_dir: &Path,
        cap_table: capnp::struct_list::Reader<'_, cap_expr::Owned>,
    ) -> Result<()> {
        let instance_id = id.id(&self.root.db)?;
        self.instances
            .get(&instance_id)
            .ok_or(Error::ModuleNotFound(instance_id))?
            .state
            .store(
                ModuleState::Initialized as u8,
                std::sync::atomic::Ordering::Release,
            );

        let span = tracing::debug_span!(
            "Initializing",
            name = self.instances.get(&instance_id).unwrap().name
        );
        let _enter = span.enter();
        tracing::debug!("Set Initialized flag");

        let program = match self
            .load_module(&id.to_string(), instance_id, config, config_dir)
            .await
        {
            Ok(program) => program,
            Err(e) => {
                return Self::failed_start(
                    &self.instances,
                    instance_id,
                    capnp::Error::failed(format!("Failed to load module program! {e}")),
                );
            }
        };

        let (conf, workpath) = match Keystone::extract_config_pair(config) {
            Ok((c, d)) => (c, d),
            Err(e) => {
                return Self::failed_start(&self.instances, instance_id, e);
            }
        };

        // Then we get a path to our current app dir
        let workpath = config_dir.join(workpath.parent().unwrap_or(workpath));
        let dir = match cap_std::fs::Dir::open_ambient_dir(
            &workpath,
            self.root.file_server.as_ref().borrow().authority,
        ) {
            Ok(x) => x,
            Err(e) => {
                return Self::failed_start(&self.instances, instance_id, e.into());
            }
        };

        let dirclient = AmbientAuthorityImpl::new_dir(&self.root.file_server, dir);

        // Pass our pair of arguments to the spawn request
        let mut spawn_request = program.spawn_request();
        let mut builder = spawn_request.get();
        builder.set_id(self.root.id_set.borrow_mut().new_client(id));
        let replacement = CapReplacement::new(conf, |index, mut builder| {
            if Self::autoinit_check(
                cap_table,
                builder.reborrow(),
                index as u32,
                instance_id,
                self.root.db.clone(),
                self.root.cells.clone(),
            )? {
                Ok(())
            } else {
                self.resolve_cap_expr(cap_table.get(index as u32), cap_table, builder)
            }
        });

        let mut pair = builder.init_args();
        let mut anybuild: capnp::any_pointer::Builder = pair.reborrow().init_config();
        if let Err(e) = anybuild.set_as(replacement) {
            return Self::failed_start(&self.instances, instance_id, e);
        }
        pair.set_aux(dirclient)?;

        tracing::debug!("Sending spawn request inside {}", workpath.display());
        let response = spawn_request.send().promise.await;
        let process = match Self::process_spawn_request(response) {
            Ok(x) => x,
            Err(e) => {
                return Self::failed_start(&self.instances, instance_id, e);
            }
        };

        tracing::debug!("Sending API request");
        let module = self
            .instances
            .get(&instance_id)
            .ok_or(Error::ModuleNotFound(instance_id))?;
        let p = process.get_api_request().send();
        capnp_rpc::queued::ClientInner::resolve(
            &module.api.inner,
            Ok(p.pipeline.get_api().as_cap()),
        );

        Ok(())
    }

    pub fn gen_id(
        &self,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
        log_filter: String,
        finisher: oneshot::Sender<crate::ModulePair>,
    ) -> Result<IdentifierImpl> {
        let name = config.get_name()?.to_string()?;
        let pause = self.instances[&IdentifierImpl::get_id(&name, &self.root.db)?]
            .pause
            .subscribe();
        Ok(IdentifierImpl::new(name, log_filter, pause, finisher))
    }

    fn default_log_env(config: &keystone_config::Reader<'_>) -> &'static str {
        match config
            .get_default_log()
            .unwrap_or(crate::keystone_capnp::LogLevel::Warning)
        {
            crate::keystone_capnp::LogLevel::None => "",
            crate::keystone_capnp::LogLevel::Trace => "trace",
            crate::keystone_capnp::LogLevel::Debug => "debug",
            crate::keystone_capnp::LogLevel::Info => "info",
            crate::keystone_capnp::LogLevel::Warning => "warn",
            crate::keystone_capnp::LogLevel::Error => "error",
        }
    }

    pub async fn init(
        &mut self,
        dir: &Path,
        config: keystone_config::Reader<'_>,
        rpc_systems: &RpcSystemSet,
    ) -> Result<()> {
        let default_log = Self::default_log_env(&config);
        let modules = config.get_modules()?;
        for s in modules.iter() {
            let (finisher, finish) = oneshot::channel();
            let id = self.gen_id(s, default_log.to_string(), finisher)?;
            let instance_id = id.id(&self.root.db)?;

            match self.init_module(id, s, dir, config.get_cap_table()?).await {
                Ok(_) => (),
                Err(e) => tracing::error!("Module Start Failure: {}", e),
            }

            let (bootstrap, rpc_future) = finish.await?;
            rpc_systems.push(rpc_future);
            let module = self
                .instances
                .get_mut(&instance_id)
                .ok_or(Error::ModuleNotFound(instance_id))?;

            module.bootstrap = Some(bootstrap);
            module.state.store(
                ModuleState::Ready as u8,
                std::sync::atomic::Ordering::Release,
            );
        }

        let stage = match std::mem::replace(&mut self.scheduler_thread, FutureStaging::None) {
            FutureStaging::None => FutureStaging::None,
            FutureStaging::Staged(pin) => FutureStaging::Running(tokio::task::spawn_local(pin)),
            FutureStaging::Running(join_handle) => FutureStaging::Running(join_handle),
        };

        self.scheduler_thread = stage;
        Ok(())
    }

    pub fn shutdown(&mut self) -> FuturesUnordered<impl Future<Output = Result<()>> + use<'_>> {
        // The scheduler thread is always running in an endless loop, so we abort it here.
        if let FutureStaging::Running(t) =
            std::mem::replace(&mut self.scheduler_thread, FutureStaging::None)
        {
            t.abort();
        }

        let set: FuturesUnordered<_> = self
            .instances
            .values_mut()
            .filter_map(|v| {
                if v.state.load(std::sync::atomic::Ordering::Acquire) != ModuleState::Closing as u8
                {
                    Some(v.stop(self.timeout))
                } else {
                    None
                }
            })
            .collect();

        set
    }

    pub fn get_api_pipe<T: FromClientHook>(&self, module: &str) -> Result<T> {
        let id = self
            .namemap
            .get(module)
            .ok_or(Error::ModuleNameNotFound(module.into()))?;
        let module = self.instances.get(id).ok_or(Error::ModuleNotFound(*id))?;

        Ok(capnp::capability::FromClientHook::new(module.api.add_ref()))
    }
    pub fn create_proxy(&self, hook: Box<dyn ClientHook>) -> capnp::capability::Client {
        self.proxy_set.borrow_mut().new_client(ProxyServer::new(
            hook,
            self.proxy_set.clone(),
            self.log.clone(),
            self.snowflake.clone(),
        ))
    }
}

pub fn fill_function_descriptions(
    cap_functions: &mut Vec<FunctionDescription>,
    dyn_schema: &capnp::schema::DynamicSchema,
    interface: capnp::schema_capnp::node::interface::Reader,
    root_id: u64,
    module_or_cap: ModuleOrCap,
    method_count: &mut u16,
) -> Result<()> {
    let methods = interface.get_methods().unwrap();
    for method in methods.into_iter() {
        let mut params = Vec::new();
        if let capnp::introspect::TypeVariant::Struct(st) = dyn_schema
            .get_type_by_id(method.get_param_struct_type())
            .unwrap()
        {
            let sc: capnp::schema::StructSchema = (*st).into();
            for field in sc.get_fields().unwrap() {
                params.push(ParamResultType {
                    name: field.get_proto().get_name().unwrap().to_string().unwrap(),
                    capnp_type: field.get_type().which().into(),
                });
            }
        };
        let mut results = Vec::new();
        if let capnp::introspect::TypeVariant::Struct(st) = dyn_schema
            .get_type_by_id(method.get_result_struct_type())
            .unwrap()
        {
            let sc: capnp::schema::StructSchema = (*st).into();
            for field in sc.get_fields().unwrap() {
                results.push(ParamResultType {
                    name: field.get_proto().get_name().unwrap().to_string().unwrap(),
                    capnp_type: field.get_type().which().into(),
                });
            }
        };
        cap_functions.push(FunctionDescription {
            module_or_cap: module_or_cap.clone(),
            function_name: method.get_name().unwrap().to_str()?.to_string(),
            type_id: root_id,
            method_id: *method_count,
            params,
            params_schema: method.get_param_struct_type(),
            results,
            results_schema: method.get_result_struct_type(),
        });
        *method_count += 1;
    }
    for s in interface.get_superclasses().unwrap().iter() {
        let schema: capnp::schema::CapabilitySchema =
            match dyn_schema.get_type_by_id(s.get_id()).unwrap() {
                capnp::introspect::TypeVariant::Capability(s) => s.to_owned().into(),
                _ => todo!(),
            };
        match schema.get_proto().which().unwrap() {
            capnp::schema_capnp::node::Which::Interface(int) => {
                fill_function_descriptions(
                    cap_functions,
                    dyn_schema,
                    int,
                    root_id,
                    module_or_cap.clone(),
                    method_count,
                )?;
            }
            _ => todo!(),
        }
    }
    Ok(())
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
        target_dir.join(format!("{name}.exe").as_str())
    }

    #[cfg(not(windows))]
    {
        target_dir.join(name)
    }
}

impl crate::keystone_capnp::root::Server for KeystoneRoot {
    async fn init_cell(
        self: Rc<Self>,
        params: crate::keystone_capnp::root::InitCellParams,
        mut results: crate::keystone_capnp::root::InitCellResults,
    ) -> Result<(), capnp::Error> {
        let span = tracing::debug_span!("host", id = BUILTIN_KEYSTONE);
        let _enter = span.enter();
        let params = params.get()?;
        tracing::debug!("init_cell()");
        let id = self
            .db
            .get_string_index(params.get_id()?.to_str()?)
            .to_capnp()?;

        let client = self
            .cells
            .borrow_mut()
            // Very important to use ::init() here so it gets initialized to a default value
            .new_client(SimpleCellImpl::init(id, self.db.clone()).to_capnp()?);

        if params.has_default() {
            self.db
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

    Ok((rpc_system, bootstrap, disconnector, api_request.send()))
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
