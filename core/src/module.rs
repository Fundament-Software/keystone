use capnp::any_pointer::Owned as any_pointer;
use capnp::capability::RemotePromise;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::{
    keystone::{Error, SpawnProcess, SpawnProgram},
    module_capnp::module_error,
    module_capnp::module_start,
};

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

impl std::fmt::Display for ModuleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleState::NotStarted => write!(f, "Not Started"),
            ModuleState::Initialized => write!(f, "Initialized"),
            ModuleState::Ready => write!(f, "Ready"),
            ModuleState::Paused => write!(f, "Paused"),
            ModuleState::Closing => write!(f, "Closing"),
            ModuleState::Closed => write!(f, "Closed"),
            ModuleState::Aborted => write!(f, "Aborted"),
            ModuleState::StartFailure => write!(f, "Start Failure"),
            ModuleState::CloseFailure => write!(f, "Close Failure"),
        }
    }
}

pub enum ModuleOrCap {
    ModuleId(u64),
    Cap(Box<dyn capnp::private::capability::ClientHook>)
}

pub struct FunctionDescription {
    pub module_or_cap: ModuleOrCap,
    pub function_name: String,
    pub type_id: u64,
    pub method_id: u16,
    pub params: std::collections::HashMap<String, capnp::introspect::TypeVariant>,
    pub params_schema: Option<capnp::schema::StructSchema>,
    pub results: std::collections::HashMap<String, capnp::introspect::TypeVariant>,
    pub results_schema: Option<capnp::schema::StructSchema>,
    pub client: Box<dyn capnp::private::capability::ClientHook>
}
//TODO potentially doesn't work for multiple of the same module
impl std::hash::Hash for FunctionDescription {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.function_name.hash(state);
        self.type_id.hash(state);
    }
}
impl PartialEq for FunctionDescription {
    fn eq(&self, other: &Self) -> bool {
        self.function_name == other.function_name && self.type_id == other.type_id
    }
}
impl Eq for FunctionDescription {}


// This can't be a rust generic because we do not know the type parameters at compile time.
pub struct ModuleInstance {
    pub module_id: u64,
    pub name: String,
    pub(crate) program: Option<SpawnProgram>,
    pub(crate) process: Option<SpawnProcess>,
    pub(crate) bootstrap: Option<module_start::Client<any_pointer, any_pointer>>,
    pub pause: tokio::sync::mpsc::Sender<bool>,
    pub api: Option<
        RemotePromise<
            crate::spawn_capnp::process::get_api_results::Owned<
                any_pointer,
                module_error::Owned<any_pointer>,
            >,
        >,
    >,
    pub state: ModuleState,
    pub queue: capnp_rpc::queued::Client,
}

impl ModuleInstance {
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

    fn reset(&mut self) {
        self.api = None;
        self.bootstrap = None;
        self.process = None;
        self.program = None;
        self.queue = capnp_rpc::queued::Client::new(None);
        let (empty_send, _) = tokio::sync::mpsc::channel(1);
        self.pause = empty_send;
    }

    pub async fn stop(&mut self, timeout: Duration) -> Result<()> {
        if self.halted() {
            return Ok(());
        }
        let _ = self.pause.send(false).await; // Ignore a failure here
        self.state = ModuleState::Closing;

        // Send the stop request to the bootstrap interface
        let Some(bootstrap) = self.bootstrap.as_ref() else {
            return Err(Error::MissingBootstrap(self.name.clone()).into());
        };

        let stop_request = bootstrap.stop_request().send();

        // Call the stop method with some timeout
        if (tokio::time::timeout(timeout, stop_request.promise).await).is_err() {
            // Force kill the module.
            self.kill().await;
            self.reset();
            Ok(())
        } else {
            if let Some(p) = self.process.as_ref() {
                // Now join the process with the same timeout
                match tokio::time::timeout(timeout, p.join_request().send().promise).await {
                    Ok(result) => {
                        self.state = match Self::check_error(&self.name, result) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::error!(
                                    "Failure during {} error lookup: {}",
                                    &self.name,
                                    e.to_string()
                                );
                                ModuleState::CloseFailure
                            }
                        };
                    }
                    Err(_) => self.kill().await,
                }
            }
            self.reset();
            Ok(())
        }
    }

    pub async fn kill(&mut self) {
        if let Some(p) = self.process.as_ref() {
            let _ = p.kill_request().send().promise.await;
        }

        self.state = ModuleState::Aborted;
    }

    pub async fn pause(&mut self, pause: bool) -> Result<()> {
        // Only change pause state if we're in a Ready or Paused state.
        if pause && self.state == ModuleState::Ready {
            self.pause.send(true).await?;
            self.state = ModuleState::Paused;
        } else if !pause && self.state == ModuleState::Paused {
            self.pause.send(false).await?;
            self.state = ModuleState::Ready;
        }
        Ok(())
    }

    fn halted(&self) -> bool {
        matches!(
            self.state,
            ModuleState::NotStarted
                | ModuleState::Closed
                | ModuleState::Aborted
                | ModuleState::StartFailure
                | ModuleState::CloseFailure
        )
    }
}
