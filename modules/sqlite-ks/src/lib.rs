use std::{path::Path, rc::Rc};

use capnp_macros::{capnp_let, capnproto_rpc};
use rusqlite::{Connection, OpenFlags, Result};
use crate::sqlite_capnp::{r_o_database, database, add_d_b};



capnp_import::capnp_import!("sqlite.capnp");

#[derive(Debug)]
struct Person {
    id: i32,
    name: String,
    data: Option<Vec<u8>>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> eyre::Result<()> {
    let db_path = "test_db";
    let (client, connection) = SqliteDatabase::new(db_path, OpenFlags::default())?;

    connection.execute(
        "CREATE TABLE person (
            id    INTEGER PRIMARY KEY,
            name  TEXT NOT NULL,
            data  BLOB
        )",
        (), // empty list of parameters.
    )?;
    let me = Person {
        id: 0,
        name: "Steven".to_string(),
        data: None,
    };
    connection.execute(
        "INSERT INTO person (name, data) VALUES (?1, ?2)",
        (&me.name, &me.data),
    )?;

    let mut stmt = connection.prepare("SELECT id, name, data FROM person")?;
    let person_iter = stmt.query_map([], |row| {
        Ok(Person {
            id: row.get(0)?,
            name: row.get(1)?,
            data: row.get(2)?,
        })
    })?;

    for person in person_iter {
        println!("Found person {:?}", person.unwrap());
    }
    Ok(())
}

struct SqliteDatabase {
    connection: Connection
}
impl SqliteDatabase {
    pub fn new_read_only<P: AsRef<Path> + Clone>(path: P, flags: OpenFlags) -> eyre::Result<(r_o_database::Client, Connection)> {
        let connection = Connection::open_with_flags(path.clone(), flags.clone())?;
        let server = SqliteDatabase{connection: connection};
        let client: r_o_database::Client = capnp_rpc::new_client(server);
        let conn = Connection::open_with_flags(path, flags)?;
        return Ok((client, conn));
    }
    pub fn new<P: AsRef<Path> + Clone>(path: P, flags: OpenFlags) -> eyre::Result<(database::Client, Connection)> {
        let connection = Connection::open_with_flags(path.clone(), flags.clone())?;
        let server = SqliteDatabase{connection: connection};
        let client: database::Client = capnp_rpc::new_client(server);
        let conn = Connection::open_with_flags(path, flags)?;
        return Ok((client, conn));
    }
    pub fn new_add_db<P: AsRef<Path> + Clone>(path: P, flags: OpenFlags) -> eyre::Result<(add_d_b::Client, Connection)> {
        let connection = Connection::open_with_flags(path.clone(), flags.clone())?;
        let server = SqliteDatabase{connection: connection};
        let client: add_d_b::Client = capnp_rpc::new_client(server);
        let conn = Connection::open_with_flags(path, flags)?;
        return Ok((client, conn));
    }
}
#[capnproto_rpc(r_o_database)]
impl r_o_database::Server for SqliteDatabase {
    async fn select(&self, q: Select) {
        capnp_let!({names, selectcore, mergeoperations, orderby, limit} = q);
        results.get();
        todo!()
    }
    async fn prepare_select(&self, q: Select) {
        results.get();
        todo!()
    }
    async fn run_prepared_select(&self, stmt: PreparedStatement<Select>, bindings: List<DBAny>) {
        results.get();
        todo!()
    }
}
#[capnproto_rpc(database)]
impl database::Server for SqliteDatabase {
    async fn insert(&self, ins: Insert) {
        results.get();
        todo!()
    }
	async fn prepare_insert(&self, ins: Insert) {
        results.get();
        todo!()
    }
    async fn run_prepared_insert (&self, stmt: PreparedStatement<Insert>, bindings: List<DBAny>) {
        results.get();
        todo!()
    }
	async fn update(&self, upd: Update) {
        todo!()
    }
	async fn prepare_update(&self, upd: Update) {
        results.get();
        todo!()
    }
	async fn run_prepared_update(&self, stmt: PreparedStatement<Update>, bindings: List<DBAny>) {
        results.get();
        todo!()
    }
	async fn delete(&self, del: Delete) {
        results.get();
        todo!()
    }
	async fn prepare_delete(&self, del: Delete) {
        results.get();
        todo!()
    }
	async fn run_prepared_delete(&self, stmt: PreparedStatement<Delete>, bindings: List<DBAny>) {
        results.get();
        todo!()
    }
}
#[capnproto_rpc(add_d_b)]
impl add_d_b::Server for SqliteDatabase {
    async fn create_table(&self, def: TableDef) {
        results.get();
        todo!()
    }
	async fn create_view(&self, names: List<Text>, def: Select) {
        results.get();
        todo!()
    }
	async fn create_restricted_table(&self, base: Table, restriction: List<TableRestriction>) {
        results.get();
        todo!()
    }
	async fn create_index(&self, base :TableRef, cols: List<IndexedColumn>, r#where: Expr) {
        results.get();
        todo!()
    }
}




#[cfg(test)]
mod tests { 
    #[tokio::test]
    async fn test() -> eyre::Result<()> {
        todo!()
    }
}