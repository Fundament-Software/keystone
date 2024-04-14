use eyre::Result;
use serde::{Deserialize, Serialize};

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

struct ModuleInstance {
    instance_id: u64,
    pid: u64,
    state: ModuleState,
}

pub trait RootDatabase {
    fn get_config(&mut self, instance_id: u64) -> Result<capnp::any_pointer::Owned>;
    fn get_sturdyref(&mut self, sturdy_id: u64) -> Result<capnp::any_pointer::Owned>;
    fn set_sturdyref(&mut self, sturdy_id: u64, data: capnp::any_pointer::Reader) -> Result<()>;
    fn add_module(&mut self, instance: &ModuleInstance) -> Result<()>;
    fn update_module(
        &mut self,
        instance_id: u64,
        pid: Option<u64>,
        state: Option<ModuleState>,
    ) -> Result<()>;
    fn clear_modules(&mut self) -> Result<()>;
}

pub trait Sqlite<DB: RootDatabase> {
    fn get_root_database(&mut self, name: &str) -> Result<DB>;
    fn create_database(&mut self, name: &str, schema: &str) -> Result<()>;
    fn delete_database(&mut self, name: &str, schema: &str) -> Result<()>;
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KnownEvent {
    Unknown,
    CreateNode,
    DestroyNode,
    CreateModule,
    DestroyModule,
    TransferModule,
    GetCapability,
    SaveCapability,
    LoadCapability,
    DropCapability,
}

type Microseconds = u64; // Microseconds since 1970 (won't overflow for 500000 years)

pub trait LogStore {
    async fn log(
        &self,
        machine: u64,
        module: u64,
        timestamp: Microseconds,
        message: &str,
    ) -> Result<()>;

    async fn log_rpc(
        &self,
        source_machine: u64,
        source_module: u64,
        target_machine: u64,
        target_module: u64,
        timestamp: Microseconds,
        interface: u64,
        payload: &[u8],
    ) -> Result<()>;

    async fn log_event(
        &self,
        machine: u64,
        module: u64,
        timestamp: Microseconds,
        event: KnownEvent,
        tag: u64,
    ) -> Result<()>;

    async fn log_structured(
        &self,
        machine: u64,
        module: u64,
        timestamp: Microseconds,
        schema: u64,
        payload: &[u8],
    ) -> Result<()>;
}
