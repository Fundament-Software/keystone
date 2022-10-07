use super::database;
use anyhow::Result;
use async_trait::async_trait;
use atomic_counter::{AtomicCounter, RelaxedCounter};
use couch_rs::document::TypedCouchDocument;
use couch_rs::types::document::DocumentId;
use couch_rs::CouchDocument;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct CouchDB {
    client: couch_rs::Client,
    rollover: RelaxedCounter,
    log: couch_rs::database::Database,
}

impl CouchDB {
    pub async fn new(host: &str, username: &str, password: &str, logdb: &str) -> Result<Self> {
        let client = couch_rs::Client::new(host, username, password)?;
        let log = match client.make_db(logdb).await {
            Ok(db) => db,
            Err(x) => client.db(logdb).await?,
        };

        Ok(Self {
            client,
            rollover: RelaxedCounter::new(0),
            log: log,
        })
    }
}

#[async_trait]
impl database::Database for couch_rs::database::Database {
    async fn store_document(&self, schema: u64, payload: &[u8]) -> Result<()> {
        Ok(())
    }
    async fn get_document(&self, name: &str) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
    async fn delete_document(&self, name: &str) -> Result<bool> {
        let doc = self.get::<Value>(name).await?;
        Ok(self.remove(&doc).await)
    }
    async fn create_view(&self, name: &str, query: &str) -> Result<()> {
        Ok(())
    }
    async fn query_view(&self, name: &str) -> Result<String> {
        Ok(String::new())
    }
    async fn destroy_view(&self, name: &str) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl database::DocumentStore for CouchDB {
    async fn create_database(&self, name: &str) -> Result<Box<dyn database::Database>> {
        let db = match self.client.make_db(name).await {
            Ok(db) => db,
            Err(x) => self.client.db(name).await?,
        };
        Ok(Box::new(db))
    }
    async fn use_database(&self, name: &str) -> Result<Box<dyn database::Database>> {
        let db = self.client.db(name).await?;

        Ok(Box::new(db))
    }
    async fn destroy_database(&self, name: &str) -> Result<bool> {
        Ok(self.client.destroy_db(name).await?)
    }
}

#[derive(Serialize, Deserialize, CouchDocument, Debug, Default)]
struct LogMessage {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub _id: DocumentId,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub _rev: String,
    pub module: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub message: String,
}

#[derive(Serialize, Deserialize, CouchDocument, Debug, Default)]
struct LogRPC {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub _id: DocumentId,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub _rev: String,
    pub interface: u64,
    pub source_module: u64,
    pub target_machine: u64,
    pub target_module: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub payload: Vec<u8>,
}

#[derive(Serialize, Deserialize, CouchDocument, Debug)]
struct LogEvent {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub _id: DocumentId,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub _rev: String,
    pub module: u64,
    pub event: database::KnownEvent,
    pub tag: u64,
}

#[async_trait]
impl database::LogStore for CouchDB {
    async fn log(&self, machine: u64, module: u64, timestamp: u64, message: &str) -> Result<()> {
        self.rollover.inc();
        let id = format!("{timestamp}{machine}{}", self.rollover.get());
        let mut obj = LogMessage {
            _rev: format!("1-{id}"),
            _id: id,
            module,
            message: message.to_string(),
        };
        self.log.save(&mut obj).await?;
        Ok(())
    }

    async fn log_rpc(
        &self,
        source_machine: u64,
        source_module: u64,
        target_machine: u64,
        target_module: u64,
        timestamp: u64,
        interface: u64,
        payload: &[u8],
    ) -> Result<()> {
        self.rollover.inc();
        let id = format!("{timestamp}{source_machine}{}", self.rollover.get());
        let mut obj = LogRPC {
            _rev: format!("1-{id}"),
            _id: id,
            source_module,
            target_machine,
            target_module,
            interface,
            payload: payload.to_vec(),
        };
        self.log.save(&mut obj).await?;
        Ok(())
    }

    async fn log_event(
        &self,
        machine: u64,
        module: u64,
        timestamp: u64,
        event: database::KnownEvent,
        tag: u64,
    ) -> Result<()> {
        self.rollover.inc();
        let id = format!("{timestamp}{machine}{}", self.rollover.get());
        let mut obj = LogEvent {
            _rev: format!("1-{id}"),
            _id: id,
            module,
            event,
            tag,
        };
        self.log.save(&mut obj).await?;
        Ok(())
    }

    async fn log_structured(
        &self,
        machine: u64,
        module: u64,
        timestamp: u64,
        schema: u64,
        payload: &[u8],
    ) -> Result<()> {
        self.rollover.inc();
        Ok(())
    }
}
