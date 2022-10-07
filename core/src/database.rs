use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait Database {
    async fn store_document(&self, schema: u64, payload: &[u8]) -> Result<()>;
    async fn get_document(&self, id: &str) -> Result<Vec<u8>>;
    async fn delete_document(&self, id: &str) -> Result<bool>;
    async fn create_view(&self, name: &str, query: &str) -> Result<()>;
    async fn query_view(&self, name: &str) -> Result<String>;
    async fn destroy_view(&self, name: &str) -> Result<()>;
}

#[async_trait]
pub trait DocumentStore {
    async fn create_database(&self, name: &str) -> Result<Box<dyn Database>>;
    async fn use_database(&self, name: &str) -> Result<Box<dyn Database>>;
    async fn destroy_database(&self, name: &str) -> Result<bool>;
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

#[async_trait]
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
