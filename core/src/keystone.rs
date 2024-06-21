use crate::cap_replacement::CapReplacement;
use crate::keystone_capnp::cap_expr;
use crate::proxy::ProxyServer;
use caplog::{CapLog, MAX_BUFFER_SIZE};
use capnp::any_pointer::Owned as any_pointer;
use capnp::capability::RemotePromise;
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
use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unexpected end of file while trying to read section[{0},{1}]")]
    UnexpectedEof(usize, usize),
    #[error("Couldn't find any embedded schema data")]
    NoSchemaData,
    #[error("File is not a valid compiled schema.")]
    NotValidSchema,
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
    db: crate::database::RootDatabase,
    log: CapLog<MAX_BUFFER_SIZE>,
    file_server: Rc<RefCell<AmbientAuthorityImpl>>,
    pub modules: HashMap<u64, ModuleInstance>,
    pub namemap: HashMap<String, u64>,
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
            db,
            log: CapLog::<MAX_BUFFER_SIZE>::new(
                caplog_config.get_max_file_size(),
                &Path::new(caplog_config.get_trie_file()?.to_str()?),
                &Path::new(caplog_config.get_data_prefix()?.to_str()?),
                caplog_config.get_max_open_files() as usize,
                check_consistency,
            )?,
            file_server: Rc::new(RefCell::new(AmbientAuthorityImpl::new())),
            modules,
            timeout: Duration::from_millis(config.get_ms_timeout()),
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

        Ok(Self::wrap_posix(spawn_process_client).await?)
    }

    #[inline]
    fn extract_config_pair<'a>(
        config: keystone_config::module_config::Reader<'a, any_pointer>,
    ) -> Result<(capnp::any_pointer::Reader<'a>, &'a Path), capnp::Error> {
        Ok((
            config.get_config()?,
            Path::new(config.get_path()?.to_str()?),
        ))
    }

    fn process_spawn_request(
        x: Result<
            capnp::capability::Response<
                crate::spawn_capnp::program::spawn_results::Owned<
                    posix_module_args::Owned<any_pointer>,
                    any_pointer,
                    module_error::Owned<any_pointer>,
                >,
            >,
            capnp::Error,
        >,
    ) -> Result<SpawnProcess, capnp::Error> {
        Ok(x?.get()?.get_result()?)
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
        let module = modules
            .get_mut(&id)
            .ok_or(eyre::eyre!("Couldn't find module!"))?;
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
            .ok_or(eyre::eyre!("Couldn't find module!"))?
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
            let replacement = CapReplacement::new(conf, |index, builder| {
                self.resolve_cap_expr(cap_table.get(index), cap_table, builder)
            });

            let mut pair = builder.init_args();
            let mut anybuild: capnp::any_pointer::Builder = pair.reborrow().init_config().into();
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

            let module = self
                .modules
                .get_mut(&id)
                .ok_or(eyre::eyre!("Couldn't find module!"))?;
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
        &mut self,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
    ) -> Result<u64> {
        Ok(self.db.get_string_index(config.get_name()?.to_str()?)? as u64)
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
    pub async fn stop_module(module: &mut ModuleInstance, timeout: Duration) -> Result<()> {
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
        if let Err(_) = tokio::time::timeout(timeout, stop_request.promise).await {
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
            let _ = Self::stop_module(v, self.timeout).await;
        }
    }
}

#[cfg(test)]
use tempfile::NamedTempFile;

#[test]
fn test_hello_world_init() -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();

    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = keystone_util::build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(
        format!(
            r#"

[[modules]]
name = "Hello World"
path = "{}"
config = {{ greeting = "Bonjour" }}

"#,
            keystone_util::get_binary_path("hello-world-module")
                .as_os_str()
                .to_str()
                .unwrap()
                .replace('\\', "/")
        )
        .as_str(),
    );

    crate::config::to_capnp(&source.parse::<toml::Table>()?, msg.reborrow())?;

    let mut instance = Keystone::new(
        message.get_root_as_reader::<keystone_config::Reader>()?,
        false,
    )?;

    let pool = tokio::task::LocalSet::new();
    let fut = pool.run_until(async_backtrace::location!().frame(async {
        instance
            .init(
                message
                    .get_root_as_reader::<keystone_config::Reader>()
                    .unwrap(),
            )
            .await
            .unwrap();

        let module = &instance.modules[&instance.namemap["Hello World"]];
        let pipe = module.api.as_ref().unwrap().pipeline.get_api().as_cap();

        let hello_client: crate::hello_world_capnp::root::Client =
            capnp::capability::FromClientHook::new(pipe);

        let mut sayhello = hello_client.say_hello_request();
        sayhello.get().init_request().set_name("Keystone".into());
        let hello_response = sayhello.send().promise.await.unwrap();

        let msg = hello_response
            .get()
            .unwrap()
            .get_reply()
            .unwrap()
            .get_message()
            .unwrap();

        assert_eq!(msg, "Bonjour, Keystone!");

        instance.shutdown().await;
    }));

    tokio::runtime::Runtime::new()?.block_on(fut);

    Ok(())
}

#[test]
fn test_hello_world_proxy() -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();

    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = keystone_util::build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(
        format!(
            r#"

[[modules]]
name = "Hello World"
path = "{}"
config = {{ greeting = "Bonjour" }}

"#,
            keystone_util::get_binary_path("hello-world-module")
                .as_os_str()
                .to_str()
                .unwrap()
                .replace('\\', "/")
        )
        .as_str(),
    );

    crate::config::to_capnp(&source.parse::<toml::Table>()?, msg.reborrow())?;

    let mut instance = Keystone::new(
        message.get_root_as_reader::<keystone_config::Reader>()?,
        false,
    )?;

    let pool = tokio::task::LocalSet::new();
    let fut = pool.run_until(async_backtrace::location!().frame(async {
        instance
            .init(
                message
                    .get_root_as_reader::<keystone_config::Reader>()
                    .unwrap(),
            )
            .await
            .unwrap();

        let module = &instance.modules[&instance.namemap["Hello World"]];
        let pipe = instance
            .proxyset
            .borrow_mut()
            .new_client(ProxyServer::new(
                module.queue.add_ref(),
                instance.proxyset.clone(),
            ))
            .hook;

        println!(
            "brand queue: {}",
            module.queue.get_resolved().unwrap().get_brand()
        );
        let hello_client: crate::hello_world_capnp::root::Client =
            capnp::capability::FromClientHook::new(pipe);

        println!("hello_client : {}", hello_client.client.hook.get_brand());

        let mut sayhello = hello_client.say_hello_request();
        sayhello.get().init_request().set_name("Keystone".into());
        let hello_response = sayhello.send().promise.await.unwrap();

        let msg = hello_response
            .get()
            .unwrap()
            .get_reply()
            .unwrap()
            .get_message()
            .unwrap();

        assert_eq!(msg, "Bonjour, Keystone!");

        instance.shutdown().await;
    }));

    tokio::runtime::Runtime::new()?.block_on(fut);

    Ok(())
}
