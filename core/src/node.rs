use super::database;
use anyhow::Result;
use std::collections::HashMap;

pub struct Node<'a> {
    id: u64,
    db: &'a dyn database::DocumentStore,
    log: &'a dyn database::LogStore,
    coredb: Box<dyn database::Database>,
    modules: HashMap<u64, u64>,
}

impl<'a> Node<'a> {
    pub async fn new(
        id: u64,
        db: &'a dyn database::DocumentStore,
        log: &'a dyn database::LogStore,
        coredb: &str,
    ) -> Result<Node<'a>> {
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
