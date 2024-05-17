use caplog::{CapLog, MAX_BUFFER_SIZE};
use capnp::any_pointer::Owned as any_pointer;
use capnp::capability::RemotePromise;
use eyre::Result;
use std::{cell::RefCell, collections::HashMap, marker::PhantomData, path::Path, rc::Rc};

use crate::{
    cap_std_capnproto::{self, AmbientAuthorityImpl},
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
use capnp_macros::capnproto_rpc;
use serde::{Deserialize, Serialize};

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
    client: Option<SpawnProgram>,
    process: Option<SpawnProcess>,
    api: Option<
        RemotePromise<
            crate::spawn_capnp::process::get_api_results::Owned<
                any_pointer,
                module_error::Owned<any_pointer>,
            >,
        >,
    >,
    state: ModuleState,
}

pub struct Keystone {
    db: crate::database::RootDatabase,
    log: CapLog<MAX_BUFFER_SIZE>,
    file_server: Rc<RefCell<AmbientAuthorityImpl>>,
    modules: HashMap<u64, ModuleInstance>,
}

impl Keystone {
    pub fn new(config: keystone_config::Reader, check_consistency: bool) -> Result<Self> {
        let caplog_config = config.get_caplog()?;
        let mut db: RootDatabase = crate::database::Manager::open_database(
            Path::new(config.get_database()?.to_str()?),
            crate::database::OpenOptions::Create,
        )?;
        let modules = config.get_modules().map_or(HashMap::new(), |modules| {
            modules
                .iter()
                .flat_map(|s| -> Result<(u64, ModuleInstance)> {
                    let id = db.get_string_index(s.get_name()?.to_str()?)? as u64;
                    Ok((
                        id,
                        ModuleInstance {
                            instance_id: id,
                            client: None,
                            process: None,
                            api: None,
                            state: ModuleState::NotStarted,
                        },
                    ))
                })
                .collect()
        });

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
    ) -> Result<(capnp::any_pointer::Reader<'a>, &'a Path)> {
        Ok((
            config.get_config()?,
            Path::new(config.get_path()?.to_str()?),
        ))
    }

    async fn init_module(
        &mut self,
        id: u64,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
    ) -> Result<()> {
        let module = self
            .modules
            .get_mut(&id)
            .ok_or(eyre::eyre!("Couldn't find module!"))?;
        module.client = Self::posix_spawn(config).await.ok();
        module.state = ModuleState::Initialized;

        let mut msg = capnp::message::Builder::new_default();
        // Build our posix_module_args, first by setting the config
        let mut pair: posix_module_args::Builder<any_pointer> = msg.init_root();

        let (conf, workpath) = match Self::extract_config_pair(config) {
            Ok((c, d)) => (c, d),
            Err(e) => {
                module.state = ModuleState::StartFailure;
                return Err(e.into());
            }
        };

        if let Err(e) = pair.set_config(conf) {
            module.state = ModuleState::StartFailure;
            return Err(e.into());
        }
        // Then we get a path to our current app dir
        let workpath = workpath.parent().unwrap_or(workpath);
        let dir = match cap_std::fs::Dir::open_ambient_dir(
            workpath,
            self.file_server.as_ref().borrow().authority,
        ) {
            Ok(x) => x,
            Err(e) => {
                module.state = ModuleState::StartFailure;
                return Err(e.into());
            }
        };

        let dirclient = AmbientAuthorityImpl::new_dir(&self.file_server, dir);
        pair.set_workdir(dirclient);

        // Pass our pair of arguments to the spawn request
        if let Some(client) = module.client.as_ref() {
            let mut spawn_request = client.spawn_request();
            let mut builder = spawn_request.get();
            if let Err(e) = builder.set_args(pair.into_reader()) {
                module.state = ModuleState::StartFailure;
                return Err(e.into());
            }

            let response = match spawn_request.send().promise.await {
                Ok(x) => x,
                Err(e) => {
                    module.state = ModuleState::StartFailure;
                    return Err(e.into());
                }
            };

            module.process = Some(response.get()?.get_result()?);

            if let Some(process) = module.process.as_ref() {
                module.api = Some(process.get_api_request().send());
                module.state = ModuleState::Ready;
            }
        } else {
            module.state = ModuleState::StartFailure;
            return Err(eyre::eyre!("Failed to acquire client!"));
        }

        Ok(())
    }

    fn get_id(
        &mut self,
        config: keystone_config::module_config::Reader<'_, any_pointer>,
    ) -> Result<u64> {
        Ok(self.db.get_string_index(config.get_name()?.to_str()?)? as u64)
    }

    pub async fn init(&mut self, config: keystone_config::Reader<'_>) -> Result<()> {
        let modules = config.get_modules()?;
        for s in modules.iter() {
            let iderr = self.get_id(s);
            if let Ok(id) = iderr {
                if let Err(e) = self.init_module(id, s).await {
                    // TODO: log error
                }
            }
        }

        Ok(())
    }
}
