use crate::database::DocumentStore;

use super::database;
use anyhow::Result;
use std::collections::HashMap;

pub struct Node<'a, T: database::DocumentStore, U: database::LogStore> {
    id: u64,
    db: &'a T,
    log: &'a U,
    coredb: T::DB,
    modules: HashMap<u64, u64>,
}

impl<'a, T: database::DocumentStore, U: database::LogStore> Node<'a, T, U> {
    pub async fn new(id: u64, db: &'a T, log: &'a U, coredb: &str) -> Result<Node<'a, T, U>> {
        let coredb = db.create_database(coredb).await?;

        // Load module metadata
        let modules = HashMap::new();
        // Resolve module graph
        // Begin bootstrapping modules
        // Store all module capnp schemas, using both module id and schema id as the keys
        Ok(Self {
            id,
            db,
            log,
            coredb,
            modules,
        })
    }
}
