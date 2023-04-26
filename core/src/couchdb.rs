use super::database;
use eyre::Result;
use async_trait::async_trait;
use atomic_counter::{AtomicCounter, RelaxedCounter};
use capnp_rpc::pry;
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

impl crate::database_capnp::connection::Server for CouchDB {
    fn create_database(
        &mut self,
        params: crate::database_capnp::connection::CreateDatabaseParams,
        results: crate::database_capnp::connection::CreateDatabaseResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let args = pry!(params.get());
        let name = args.get_name().unwrap();

        /*Promise::from_future(
            async move {
                let db = match self.client.make_db(name).await {
                    Ok(db) => db,
                    Err(x) => self.client.db(name).await?,
                };
                //results.get().set_database(db);
                Ok(())
            }
            .map_err(|e: Box<dyn std::error::Error>| Error::failed(format!("{:?}", e))),
        )*/
        capnp::capability::Promise::err(capnp::Error::unimplemented(
            "method connection::Server::create_database not implemented".to_string(),
        ))
    }

    fn get_database(
        &mut self,
        _: crate::database_capnp::connection::GetDatabaseParams,
        _: crate::database_capnp::connection::GetDatabaseResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        capnp::capability::Promise::err(capnp::Error::unimplemented(
            "method connection::Server::get_database not implemented".to_string(),
        ))
    }

    fn destroy_database(
        &mut self,
        _: crate::database_capnp::connection::DestroyDatabaseParams,
        _: crate::database_capnp::connection::DestroyDatabaseResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        capnp::capability::Promise::err(capnp::Error::unimplemented(
            "method connection::Server::destroy_database not implemented".to_string(),
        ))
    }
}

#[async_trait]
impl database::Database for couch_rs::database::Database {
    async fn store_document(&self, schema: u64, payload: &[u8]) -> Result<()> {
        //self.upsert()
        Ok(())
    }
    async fn store_struct<T: std::marker::Send>(&self, data: T) -> Result<()> {
        Ok(())
    }
    async fn get_document(&self, name: &str) -> Result<Vec<u8>> {
        //let doc = self.get::<CouchDocument>(name).await?;

        Ok(Vec::new())
    }
    async fn get_struct<T: Default>(&self, id: &str) -> Result<T> {
        //self.upsert()
        let a = T::default();
        Ok(a)
    }
    async fn delete_document(&self, name: &str) -> Result<bool> {
        let doc: Value = self.get(name).await?;
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
    type DB = couch_rs::database::Database;

    async fn create_database(&self, name: &str) -> Result<Self::DB> {
        let db = self.client.make_db(name).await?;

        Ok(db)
    }
    async fn use_database(&self, name: &str) -> Result<Self::DB> {
        let db = self.client.db(name).await?;

        Ok(db)
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
