use std::{cell::RefCell, ops::AddAssign, path::Path, rc::Rc};

use crate::sqlite_capnp::{add_d_b, database, r_o_database};
use capnp::capability::FromClientHook;
use capnp_macros::{capnp_build, capnp_let, capnproto_rpc};
use capnp_rpc::CapabilityServerSet;
use rusqlite::{
    params, params_from_iter, types::ToSqlOutput, Connection, OpenFlags, Result, ToSql,
};
use sqlite_capnp::{
    d_b_any, delete, function_invocation, insert, prepared_statement, r_a_table_ref, r_o_table_ref,
    result_stream, select, sql_function, table, table_ref, update,
};

capnp_import::capnp_import!("sqlite.capnp");

enum dbany {
    None,
    int(i64),
    real(f64),
    str(String),
    blob(Vec<u8>),
}
//TODO make a real result stream
struct PlaceholderResults {
    buffer: Vec<Vec<dbany>>,
}
#[capnproto_rpc(result_stream)]
impl result_stream::Server for PlaceholderResults {
    async fn next(&self, size: u16) {
        //TODO Size is supposed to be next n I guess?
        let mut builder = results.get().init_res();
        builder.set_finished(true);
        let mut results_builder = builder.init_results(self.buffer.len() as u32);
        for i in 0..self.buffer.len() {
            let mut dbany_builder = results_builder
                .reborrow()
                .init(i as u32, self.buffer[i].len() as u32);
            for j in 0..self.buffer[i].len() {
                match &self.buffer[i][j] {
                    dbany::None => dbany_builder.reborrow().get(j as u32).set_null(()),
                    dbany::int(i) => dbany_builder
                        .reborrow()
                        .get(j as u32)
                        .set_integer(i.clone()),
                    dbany::real(r) => dbany_builder.reborrow().get(j as u32).set_real(r.clone()),
                    dbany::str(str) => dbany_builder
                        .reborrow()
                        .get(j as u32)
                        .set_text(str.as_str().into()),
                    dbany::blob(blob) => dbany_builder.reborrow().get(j as u32).set_blob(blob),
                }
            }
        }

        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> eyre::Result<()> {
    let db_path = "test_db";
    let (client, connection) = SqliteDatabase::new(db_path, OpenFlags::default())?;

    //connection.execute("CREATE TABLE person (id INTEGER PRIMARY KEY, name TEXT NOT NULL)", ())?;

    let mut create_table_request = client.create_table_request();
    let mut table_def_builder = create_table_request.get().init_def();
    let mut field_builder = table_def_builder.init_fields(2);
    let mut first_field_builder = field_builder.reborrow().get(0);
    first_field_builder.set_name("name".into());
    first_field_builder.set_base_type(sqlite_capnp::table_field::Type::Text);
    first_field_builder.set_nullable(false);
    let mut second_field_builder = field_builder.reborrow().get(1);
    second_field_builder.set_name("data".into());
    second_field_builder.set_base_type(sqlite_capnp::table_field::Type::Blob);
    second_field_builder.set_nullable(true);

    let table_cap = create_table_request
        .send()
        .promise
        .await?
        .get()?
        .get_res()?;

    let ra_table_ref_cap = table_cap
        .adminless_request()
        .send()
        .promise
        .await?
        .get()?
        .get_res()?
        .appendonly_request()
        .send()
        .promise
        .await?
        .get()?
        .get_res()?;

    //connection.execute(
    //    "INSERT INTO person (name, data) VALUES (?1, ?2)",
    //    (&me.name, &me.data),
    //)?;
    /*
    connection.execute(
        "INSERT INTO person (name) VALUES ('Steven')",
        (),
    )?;*/

    let mut insert_request = client
        .clone()
        .cast_to::<database::Client>()
        .insert_request();
    let mut builder = insert_request.get().init_ins();
    builder.set_fallback(sqlite_capnp::insert::ConflictStrategy::Fail);
    builder.set_target(ra_table_ref_cap.clone());
    let mut cols_builder = builder.reborrow().init_cols(2);
    cols_builder.set(0, "name".into());
    cols_builder.set(1, "data".into());
    let mut source_builder = builder.reborrow().init_source();
    let mut inner_list_builder = source_builder.init_values(1).init(0, 2);
    inner_list_builder
        .reborrow()
        .get(0)
        .set_text("Steven".into());
    inner_list_builder.reborrow().get(1).set_null(());
    //builder.set_returning();
    insert_request.send().promise.await?;

    let mut prepare_insert_request = client
        .clone()
        .cast_to::<database::Client>()
        .prepare_insert_request();
    let mut builder = prepare_insert_request.get().init_ins();
    builder.set_fallback(sqlite_capnp::insert::ConflictStrategy::Fail);
    builder.set_target(ra_table_ref_cap.clone());
    let mut cols_builder = builder.reborrow().init_cols(2);
    cols_builder.set(0, "name".into());
    cols_builder.set(1, "data".into());
    let mut source_builder = builder.reborrow().init_source();
    let mut inner_list_builder = source_builder.init_values(1).init(0, 2);
    inner_list_builder.reborrow().get(0).set_text("Mike".into());
    inner_list_builder.reborrow().get(1).set_blob(&[1, 2, 3]);
    builder.reborrow().init_returning(1).get(0).set_bindparam(0);
    //builder.set_returning();
    let prepared = prepare_insert_request
        .send()
        .promise
        .await?
        .get()?
        .get_stmt()?;
    let mut run_request = client
        .clone()
        .cast_to::<database::Client>()
        .run_prepared_insert_request();
    run_request.get().set_stmt(prepared);
    run_request
        .get()
        .init_bindings(1)
        .get(0)
        .set_text("meow".into());
    run_request.send().promise.await?;

    let mut select_request = client
        .clone()
        .cast_to::<r_o_database::Client>()
        .select_request();
    let mut builder = select_request.get().init_q();
    let mut select_core_builder = builder.reborrow().init_selectcore();
    let mut res_select_core_builder = select_core_builder.reborrow().init_result(3);
    res_select_core_builder
        .reborrow()
        .get(0)
        .init_literal()
        .set_text("id".into());
    res_select_core_builder
        .reborrow()
        .get(1)
        .init_literal()
        .set_text("name".into());
    res_select_core_builder
        .reborrow()
        .get(2)
        .init_literal()
        .set_text("data".into());
    /*let mut zero = res_select_core_builder.reborrow().get(0).init_tablereference();
    zero.set_col_name("id".into());
    zero.set_reference(0);
    let mut one = res_select_core_builder.reborrow().get(1).init_tablereference();
    one.set_col_name("name".into());
    one.set_reference(0);
    let mut two = res_select_core_builder.reborrow().get(2).init_tablereference();
    two.set_col_name("data".into());
    two.set_reference(0);*/
    let ro_tableref_cap = ra_table_ref_cap
        .readonly_request()
        .send()
        .promise
        .await?
        .get()?
        .get_res()?;
    select_core_builder
        .init_from()
        .init_tableorsubquery()
        .set_tableref(ro_tableref_cap);

    let mut res_stream = select_request.send().promise.await?.get()?.get_res()?;
    let next_request = res_stream.next_request();
    let res = next_request.send().promise.await?;
    let rows = res.get()?.get_res()?.get_results()?;
    for row in rows.iter() {
        for value in row?.iter() {
            match value.which()? {
                d_b_any::Which::Null(()) => print!("None "),
                d_b_any::Which::Integer(int) => print!("{int} "),
                d_b_any::Which::Real(real) => print!("{real} "),
                d_b_any::Which::Text(text) => print!("{} ", text?.to_str()?),
                d_b_any::Which::Blob(blob) => print!("{} ", std::str::from_utf8(blob?)?),
                d_b_any::Which::Pointer(_) => todo!(),
            }
        }
        println!();
    }

    let mut delete_from_table_request = client
        .clone()
        .cast_to::<database::Client>()
        .delete_request();
    let mut builder = delete_from_table_request.get().init_del();
    let table_ref = table_cap
        .adminless_request()
        .send()
        .promise
        .await?
        .get()?
        .get_res()?;
    builder.set_from(table_ref);
    //delete_from_table_request.send().promise.await?;
    //TODO Test where clause and returning

    Ok(())
}

struct SqliteDatabase {
    connection: Connection,
}
impl SqliteDatabase {
    pub fn new_read_only<P: AsRef<Path> + Clone>(
        path: P,
        flags: OpenFlags,
    ) -> eyre::Result<(r_o_database::Client, Connection)> {
        let connection = Connection::open_with_flags(path.clone(), flags.clone())?;
        let server = SqliteDatabase {
            connection: connection,
        };
        let client: r_o_database::Client = capnp_rpc::new_client(server);
        let conn = Connection::open_with_flags(path, flags)?;
        return Ok((client, conn));
    }
    pub fn new<P: AsRef<Path> + Clone>(
        path: P,
        flags: OpenFlags,
    ) -> eyre::Result<(add_d_b::Client, Connection)> {
        let connection = Connection::open_with_flags(path.clone(), flags.clone())?;
        let server = SqliteDatabase {
            connection: connection,
        };
        let client: add_d_b::Client = capnp_rpc::new_client(server);
        let conn = Connection::open_with_flags(path, flags)?;
        return Ok((client, conn));
    }
    pub fn new_add_db<P: AsRef<Path> + Clone>(
        path: P,
        flags: OpenFlags,
    ) -> eyre::Result<(add_d_b::Client, Connection)> {
        let connection = Connection::open_with_flags(path.clone(), flags.clone())?;
        let server = SqliteDatabase {
            connection: connection,
        };
        let client: add_d_b::Client = capnp_rpc::new_client(server);
        let conn = Connection::open_with_flags(path, flags)?;
        return Ok((client, conn));
    }
}

#[capnproto_rpc(r_o_database)]
impl r_o_database::Server for SqliteDatabase {
    async fn select(&self, q: Select) {
        let statement_and_params =
            build_select_statement(q, String::new(), Vec::new(), Vec::new()).unwrap();
        //TODO Placeholder results
        let mut stmt = self
            .connection
            .prepare(statement_and_params.statement.as_str())
            .unwrap();
        println!("{}", statement_and_params.statement); //Debugging
        println!("{:?}", statement_and_params.params);
        let mut rows = stmt
            .query(params_from_iter(statement_and_params.params.iter()))
            .unwrap();
        let mut row_vec = Vec::new();

        //TODO figure out results stream
        while let Ok(Some(row)) = rows.next() {
            let mut value_vec = Vec::new();
            let mut i = 0;
            while let Ok(value) = row.get_ref(i) {
                match value {
                    rusqlite::types::ValueRef::Null => value_vec.push(dbany::None),
                    rusqlite::types::ValueRef::Integer(int) => value_vec.push(dbany::int(int)),
                    rusqlite::types::ValueRef::Real(r) => value_vec.push(dbany::real(r)),
                    rusqlite::types::ValueRef::Text(t) => {
                        value_vec.push(dbany::str(std::str::from_utf8(t)?.to_string()))
                    }
                    rusqlite::types::ValueRef::Blob(b) => value_vec.push(dbany::blob(b.to_vec())),
                }
                i += 1;
            }
            row_vec.push(value_vec)
        }
        results
            .get()
            .set_res(capnp_rpc::new_client(PlaceholderResults {
                buffer: row_vec,
            }));
        Ok(())
    }
    async fn prepare_select(&self, q: Select) {
        let statement_and_params =
            build_select_statement(q, String::new(), Vec::new(), Vec::new()).unwrap();
        let client = PREPARED_SELECT_SET.with_borrow_mut(|set| {
            return set.new_client(statement_and_params);
        });
        results.get().set_stmt(client);
        Ok(())
    }
    async fn run_prepared_select(&self, stmt: PreparedStatement<Select>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let mut statement_and_params = PREPARED_SELECT_SET.with_borrow_mut(|set| {
            let Some(server) = set.get_local_server_of_resolved(&resolved) else {
                return Err(capnp::Error::failed(
                    "Prepared statement doesn't exist, or was created on a different machine"
                        .to_string(),
                ));
            };
            Ok(server)
        })?;
        //Not sure if prepare_cached is good enough, alternatively could store an actual prepared statement(As well as avoid some cloning) but requires more lifetime stuff
        let mut prepared = self
            .connection
            .prepare_cached(statement_and_params.statement.as_str())
            .unwrap();
        let mut bindings_iter = bindings.iter();
        let mut params = statement_and_params.params.clone();
        for index in &statement_and_params.bindparam_indexes {
            if let Some(param) = bindings_iter.next() {
                params[*index] = match param.which()? {
                    d_b_any::Which::Null(_) => rusqlite::types::Value::Null,
                    d_b_any::Which::Integer(i) => rusqlite::types::Value::Integer(i),
                    d_b_any::Which::Real(r) => rusqlite::types::Value::Real(r),
                    d_b_any::Which::Text(text) => rusqlite::types::Value::Text(text?.to_string()?),
                    d_b_any::Which::Blob(blob) => rusqlite::types::Value::Blob(blob?.to_vec()),
                    d_b_any::Which::Pointer(_) => todo!(),
                }
            } else {
                return Err(capnp::Error::failed(
                    "Not enough params provided for binding slots specified in prepare statement"
                        .to_string(),
                ));
            }
        }
        if let Some(_) = bindings_iter.next() {
            return Err(capnp::Error::failed(
                "Too many params provided for binding slots specified in prepare statement"
                    .to_string(),
            ));
        }
        let rows = prepared.query(params_from_iter(params.iter()));
        //TODO results stuff
        results.get();
        Ok(())
    }
}

#[capnproto_rpc(database)]
impl database::Server for SqliteDatabase {
    async fn insert(&self, ins: Insert) {
        let statement_and_params =
            build_insert_statement(ins, String::new(), Vec::new(), Vec::new()).unwrap();
        println!("{}", statement_and_params.statement);
        println!("{:?}", statement_and_params.params);
        //TODO result stuff
        results.get();
        self.connection
            .execute(
                statement_and_params.statement.as_str(),
                params_from_iter(statement_and_params.params.iter()),
            )
            .unwrap();
        Ok(())
    }
    async fn prepare_insert(&self, ins: Insert) {
        let statement_and_params =
            build_insert_statement(ins, String::new(), Vec::new(), Vec::new()).unwrap();
        let client = PREPARED_INSERT_SET.with_borrow_mut(|set| {
            return set.new_client(statement_and_params);
        });
        results.get().set_stmt(client);
        Ok(())
    }
    async fn run_prepared_insert(&self, stmt: PreparedStatement<Insert>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let mut statement_and_params = PREPARED_INSERT_SET.with_borrow_mut(|set| {
            let Some(server) = set.get_local_server_of_resolved(&resolved) else {
                return Err(capnp::Error::failed(
                    "Prepared statement doesn't exist, or was created on a different machine"
                        .to_string(),
                ));
            };
            Ok(server)
        })?;
        let mut prepared = self
            .connection
            .prepare_cached(statement_and_params.statement.as_str())
            .unwrap();
        let mut bindings_iter = bindings.iter();
        let mut params = statement_and_params.params.clone();
        for index in &statement_and_params.bindparam_indexes {
            if let Some(param) = bindings_iter.next() {
                params[*index] = match param.which()? {
                    d_b_any::Which::Null(_) => rusqlite::types::Value::Null,
                    d_b_any::Which::Integer(i) => rusqlite::types::Value::Integer(i),
                    d_b_any::Which::Real(r) => rusqlite::types::Value::Real(r),
                    d_b_any::Which::Text(text) => rusqlite::types::Value::Text(text?.to_string()?),
                    d_b_any::Which::Blob(blob) => rusqlite::types::Value::Blob(blob?.to_vec()),
                    d_b_any::Which::Pointer(_) => todo!(),
                }
            } else {
                return Err(capnp::Error::failed(
                    "Not enough params provided for binding slots specified in prepare statement"
                        .to_string(),
                ));
            }
        }
        if let Some(_) = bindings_iter.next() {
            return Err(capnp::Error::failed(
                "Too many params provided for binding slots specified in prepare statement"
                    .to_string(),
            ));
        }
        println!("{}", statement_and_params.statement);
        println!("{:?}", params);
        let rows = prepared.execute(params_from_iter(params.iter()));
        //self.connection.execute(statement_and_params.statement.as_str(), params_from_iter(params.iter()));
        //TODO results stuff
        results.get();
        Ok(())
    }
    async fn update(&self, upd: Update) {
        let statement_and_params =
            build_update_statement(upd, String::new(), Vec::new(), Vec::new()).unwrap();
        println!("{}", statement_and_params.statement);
        println!("{:?}", statement_and_params.params);
        //TODO result stuff
        results.get();
        self.connection
            .execute(
                statement_and_params.statement.as_str(),
                params_from_iter(statement_and_params.params.iter()),
            )
            .unwrap();
        Ok(())
    }
    async fn prepare_update(&self, upd: Update) {
        let statement_and_params =
            build_update_statement(upd, String::new(), Vec::new(), Vec::new()).unwrap();
        let client = PREPARED_UPDATE_SET.with_borrow_mut(|set| {
            return set.new_client(statement_and_params);
        });
        results.get().set_stmt(client);
        Ok(())
    }
    async fn run_prepared_update(&self, stmt: PreparedStatement<Update>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let mut statement_and_params = PREPARED_UPDATE_SET.with_borrow_mut(|set| {
            let Some(server) = set.get_local_server_of_resolved(&resolved) else {
                return Err(capnp::Error::failed(
                    "Prepared statement doesn't exist, or was created on a different machine"
                        .to_string(),
                ));
            };
            Ok(server)
        })?;
        let mut prepared = self
            .connection
            .prepare_cached(statement_and_params.statement.as_str())
            .unwrap();
        let mut bindings_iter = bindings.iter();
        let mut params = statement_and_params.params.clone();
        for index in &statement_and_params.bindparam_indexes {
            if let Some(param) = bindings_iter.next() {
                params[*index] = match param.which()? {
                    d_b_any::Which::Null(_) => rusqlite::types::Value::Null,
                    d_b_any::Which::Integer(i) => rusqlite::types::Value::Integer(i),
                    d_b_any::Which::Real(r) => rusqlite::types::Value::Real(r),
                    d_b_any::Which::Text(text) => rusqlite::types::Value::Text(text?.to_string()?),
                    d_b_any::Which::Blob(blob) => rusqlite::types::Value::Blob(blob?.to_vec()),
                    d_b_any::Which::Pointer(_) => todo!(),
                }
            } else {
                return Err(capnp::Error::failed(
                    "Not enough params provided for binding slots specified in prepare statement"
                        .to_string(),
                ));
            }
        }
        if let Some(_) = bindings_iter.next() {
            return Err(capnp::Error::failed(
                "Too many params provided for binding slots specified in prepare statement"
                    .to_string(),
            ));
        }
        let rows = prepared.query(params_from_iter(params.iter()));
        //TODO results stuff
        results.get();
        Ok(())
    }
    async fn delete(&self, del: Delete) {
        let statement_and_params =
            build_delete_statement(del, String::new(), Vec::new(), Vec::new()).unwrap();
        self.connection
            .execute(
                statement_and_params.statement.as_str(),
                params_from_iter(statement_and_params.params.iter()),
            )
            .unwrap();
        //TODO results stream
        results.get();
        Ok(())
    }
    async fn prepare_delete(&self, del: Delete) {
        let statement_and_params =
            build_delete_statement(del, String::new(), Vec::new(), Vec::new()).unwrap();
        let client = PREPARED_DELETE_SET.with_borrow_mut(|set| {
            return set.new_client(statement_and_params);
        });
        results.get().set_stmt(client);
        Ok(())
    }
    async fn run_prepared_delete(&self, stmt: PreparedStatement<Delete>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let mut statement_and_params = PREPARED_DELETE_SET.with_borrow_mut(|set| {
            let Some(server) = set.get_local_server_of_resolved(&resolved) else {
                return Err(capnp::Error::failed(
                    "Prepared statement doesn't exist, or was created on a different machine"
                        .to_string(),
                ));
            };
            Ok(server)
        })?;
        let mut prepared = self
            .connection
            .prepare_cached(statement_and_params.statement.as_str())
            .unwrap();
        let mut bindings_iter = bindings.iter();
        let mut params = statement_and_params.params.clone();
        for index in &statement_and_params.bindparam_indexes {
            if let Some(param) = bindings_iter.next() {
                params[*index] = match param.which()? {
                    d_b_any::Which::Null(_) => rusqlite::types::Value::Null,
                    d_b_any::Which::Integer(i) => rusqlite::types::Value::Integer(i),
                    d_b_any::Which::Real(r) => rusqlite::types::Value::Real(r),
                    d_b_any::Which::Text(text) => rusqlite::types::Value::Text(text?.to_string()?),
                    d_b_any::Which::Blob(blob) => rusqlite::types::Value::Blob(blob?.to_vec()),
                    d_b_any::Which::Pointer(_) => todo!(),
                }
            } else {
                return Err(capnp::Error::failed(
                    "Not enough params provided for binding slots specified in prepare statement"
                        .to_string(),
                ));
            }
        }
        if let Some(_) = bindings_iter.next() {
            return Err(capnp::Error::failed(
                "Too many params provided for binding slots specified in prepare statement"
                    .to_string(),
            ));
        }
        let rows = prepared.query(params_from_iter(params.iter()));
        //TODO results stuff
        results.get();
        Ok(())
    }
}
struct TableRefImpl {
    table_name: Rc<String>,
}
#[capnproto_rpc(r_o_table_ref)]
impl r_o_table_ref::Server for TableRefImpl {}
#[capnproto_rpc(r_a_table_ref)]
impl r_a_table_ref::Server for TableRefImpl {
    async fn readonly(&self) {
        let client: r_o_table_ref::Client = ROTABLE_REF_SET.with_borrow_mut(|set| {
            set.new_client(TableRefImpl {
                table_name: self.table_name.clone(),
            })
        });
        results.get().set_res(client);
        Ok(())
    }
}
#[capnproto_rpc(table_ref)]
impl table_ref::Server for TableRefImpl {
    async fn appendonly(&self) {
        let client: r_a_table_ref::Client = RATABLE_REF_SET.with_borrow_mut(|set| {
            set.new_client(TableRefImpl {
                table_name: self.table_name.clone(),
            })
        });
        results.get().set_res(client);
        Ok(())
    }
}
#[capnproto_rpc(table)]
impl table::Server for TableRefImpl {
    async fn adminless(&self) {
        let client: table_ref::Client = TABLE_REF_SET.with_borrow_mut(|set| {
            set.new_client(TableRefImpl {
                table_name: self.table_name.clone(),
            })
        });
        results.get().set_res(client);
        Ok(())
    }
}

#[capnproto_rpc(add_d_b)]
impl add_d_b::Server for SqliteDatabase {
    async fn create_table(&self, def: TableDef) {
        let table = generate_table_name();
        let mut statement = String::new();
        statement.push_str("CREATE TABLE ");
        statement.push_str(table.table_name.as_str());
        statement.push_str(" (");
        statement.push_str("id INTEGER PRIMARY KEY, ");
        let fields = def.get_fields()?;

        for field in fields.iter() {
            statement.push_str(field.get_name()?.to_str()?);

            match field.get_base_type()? {
                sqlite_capnp::table_field::Type::Integer => statement.push_str(" INTEGER"),
                sqlite_capnp::table_field::Type::Real => statement.push_str(" REAL"),
                sqlite_capnp::table_field::Type::Text => statement.push_str(" TEXT"),
                sqlite_capnp::table_field::Type::Blob => statement.push_str(" BLOB"),
                sqlite_capnp::table_field::Type::Pointer => todo!(),
            }
            if !field.get_nullable() {
                statement.push_str(" NOT NULL");
            }
            statement.push_str(", ");
        }
        if statement.as_bytes()[statement.len() - 2] == b',' {
            statement.truncate(statement.len() - 2);
        }
        statement.push_str(")");
        self.connection.execute(statement.as_str(), ()).unwrap();
        let table_client: table::Client = capnp_rpc::new_client(table);
        results.get().set_res(table_client);
        Ok(())
    }
    async fn create_view(&self, names: List<Text>, def: Select) {
        let mut statement = String::new();
        statement.push_str("CREATE VIEW ");
        statement.push_str(create_view_name());
        statement.push_str(" ");

        if let (Some(_)) = names.iter().next() {
            statement.push_str("(");
            for name in names.iter() {
                statement.push_str(name?.to_str()?);
                statement.push_str(", ")
            }
            if statement.as_bytes()[statement.len() - 2] == b',' {
                statement.truncate(statement.len() - 2);
            }
            statement.push_str(") ");
        }
        /* 
        statement.push_str(
            build_select_statement(def, String::new(), Vec::new(), Vec::new())
                .unwrap()
                .statement
                .as_str(),
        );*/

        //TODO run statement, turn result into rotableref

        results.get();
        todo!()
    }
    async fn create_restricted_table(&self, base: Table, restriction: List<TableRestriction>) {
        results.get();
        todo!()
    }
    async fn create_index(&self, base: TableRef, cols: List<IndexedColumn>, r#where: Expr) {
        let mut statement = String::new();
        statement.push_str("CREATE INDEX ");
        statement.push_str(create_index_name());
        statement.push_str(" ON ");
        TABLE_REF_SET.with_borrow_mut(|set| {
            let Some(server) = set.get_local_server_of_resolved(&base) else {
                return Err(capnp::Error::failed(
                    "Table ref invalid for this database or insufficient permissions".to_string(),
                ));
            };
            statement.push_str(server.table_name.as_str());
            statement.push(' ');
            Ok(())
        })?;
        let mut sql_params: Vec<rusqlite::types::Value> = Vec::new();
        let mut bindparam_indexes: Vec<usize> = Vec::new();
        statement.push_str("(");
        for index_column in cols.iter() {
            match index_column.which()? {
                sqlite_capnp::indexed_column::Which::Name(name) => {
                    statement.push_str(name?.to_str()?);
                    statement.push_str(", ");
                }
                sqlite_capnp::indexed_column::Which::Expr(expr) => match expr?.which()? {
                    sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
                        sqlite_capnp::d_b_any::Which::Null(_) => {
                            sql_params.push(rusqlite::types::Value::Null);
                            statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                        }
                        sqlite_capnp::d_b_any::Which::Integer(int) => {
                            sql_params.push(rusqlite::types::Value::Integer(int));
                            statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                        }
                        sqlite_capnp::d_b_any::Which::Real(real) => {
                            sql_params.push(rusqlite::types::Value::Real(real));
                            statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                        }
                        sqlite_capnp::d_b_any::Which::Text(text) => {
                            sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                            statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                        }
                        sqlite_capnp::d_b_any::Which::Blob(blob) => {
                            sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                            statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                        }
                        sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                            todo!()
                        }
                    },
                    sqlite_capnp::expr::Which::Bindparam(_) => {
                        sql_params.push(rusqlite::types::Value::Null);
                        bindparam_indexes.push(sql_params.len() - 1);
                        statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                    }
                    sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
                    sqlite_capnp::expr::Which::Functioninvocation(func) => {
                        let statement_and_params = build_function_invocation(
                            func?,
                            statement,
                            sql_params,
                            bindparam_indexes,
                        )?;
                        statement = statement_and_params.statement;
                        sql_params = statement_and_params.params;
                        bindparam_indexes = statement_and_params.bindparam_indexes;
                    }
                },
            }
            statement.push_str(", ");
        }
        if statement.as_bytes()[statement.len() - 2] == b',' {
            statement.truncate(statement.len() - 2);
        }
        statement.push_str(")");

        match r#where.which()? {
            sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
                sqlite_capnp::d_b_any::Which::Null(_) => {}
                sqlite_capnp::d_b_any::Which::Integer(int) => {
                    sql_params.push(rusqlite::types::Value::Integer(int));
                    statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Real(real) => {
                    sql_params.push(rusqlite::types::Value::Real(real));
                    statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Text(text) => {
                    sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                    statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Blob(blob) => {
                    sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                    statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                    todo!()
                }
            },
            sqlite_capnp::expr::Which::Bindparam(_) => {
                sql_params.push(rusqlite::types::Value::Null);
                bindparam_indexes.push(sql_params.len() - 1);
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
            sqlite_capnp::expr::Which::Functioninvocation(func) => {
                statement.push_str("WHERE ");
                let statement_and_params =
                    build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
                statement = statement_and_params.statement;
                sql_params = statement_and_params.params;
                bindparam_indexes = statement_and_params.bindparam_indexes;
            }
        }
        let mut stmt = self.connection.prepare(statement.as_str()).unwrap();
        let mut rows = stmt.query(params_from_iter(sql_params.iter())).unwrap();
        //TODO Results stuff
        results.get();
        Ok(())
    }
}
struct SqlFunction {
    function: String,
}
impl sql_function::Server for SqlFunction {}
impl prepared_statement::Server<insert::Owned> for StatementAndParams {}
impl prepared_statement::Server<select::Owned> for StatementAndParams {}
impl prepared_statement::Server<delete::Owned> for StatementAndParams {}
impl prepared_statement::Server<update::Owned> for StatementAndParams {}
//Probably put all of this along with other various local stuff in some sort of "Local" struct accessed through the keystone module
thread_local!(
    static ROTABLE_REF_SET: RefCell<CapabilityServerSet<TableRefImpl, r_o_table_ref::Client>> =
        RefCell::new(CapabilityServerSet::new());
    static RATABLE_REF_SET: RefCell<CapabilityServerSet<TableRefImpl, r_a_table_ref::Client>> =
        RefCell::new(CapabilityServerSet::new());
    static TABLE_REF_SET: RefCell<CapabilityServerSet<TableRefImpl, table_ref::Client>> =
        RefCell::new(CapabilityServerSet::new());
    static TABLE_SET: RefCell<CapabilityServerSet<TableRefImpl, table::Client>> =
        RefCell::new(CapabilityServerSet::new());
    static PREPARED_INSERT_SET: RefCell<
        CapabilityServerSet<StatementAndParams, prepared_statement::Client<insert::Owned>>,
    > = RefCell::new(CapabilityServerSet::new());
    static PREPARED_SELECT_SET: RefCell<
        CapabilityServerSet<StatementAndParams, prepared_statement::Client<select::Owned>>,
    > = RefCell::new(CapabilityServerSet::new());
    static PREPARED_DELETE_SET: RefCell<
        CapabilityServerSet<StatementAndParams, prepared_statement::Client<delete::Owned>>,
    > = RefCell::new(CapabilityServerSet::new());
    static PREPARED_UPDATE_SET: RefCell<
        CapabilityServerSet<StatementAndParams, prepared_statement::Client<update::Owned>>,
    > = RefCell::new(CapabilityServerSet::new());
    static SQL_FUNCTION_SET: RefCell<CapabilityServerSet<SqlFunction, sql_function::Client>> =
        RefCell::new(CapabilityServerSet::new());
);
fn generate_table_name() -> TableRefImpl {
    let name = format!("table{}", rand::random::<u64>().to_string());
    TableRefImpl {
        table_name: Rc::new(name),
    }
}
fn create_index_name<'a>() -> &'a str {
    //TODO Generate index names I guess
    return &"abc";
}
fn create_view_name<'a>() -> &'a str {
    return &"asd";
}
struct StatementAndParams {
    statement: String,
    params: Vec<rusqlite::types::Value>,
    bindparam_indexes: Vec<usize>,
}
//Maybe a way to get by without clonning or cached statements, but potential lifetime questions
struct PreparedStatementAndParams<'a> {
    statement: rusqlite::Statement<'a>,
    params: Vec<rusqlite::types::ValueRef<'a>>,
    bindparam_indexes: Vec<usize>,
}

fn build_insert_statement<'a>(
    ins: insert::Reader<'a>,
    mut statement: String,
    mut sql_params: Vec<rusqlite::types::Value>,
    mut bindparam_indexes: Vec<usize>,
) -> eyre::Result<StatementAndParams> {
    let fallback_reader = ins.get_fallback()?;
    capnp_let!({target, cols, returning} = ins);
    let source = ins.get_source();

    statement.push_str("INSERT OR ");
    match fallback_reader {
        abort => statement.push_str("ABORT INTO "),
        fail => statement.push_str("FAIL INTO "),
        ignore => statement.push_str("IGNORE INTO "),
        rollback => statement.push_str("ROLLBACK INTO "),
    };
    //statement.push_str("person ");
    //let target = capnp::capability::get_resolved_cap(target).await;
    RATABLE_REF_SET.with_borrow(|set| {
        let Some(server) = set.get_local_server_of_resolved(&target) else {
            return Err(capnp::Error::failed(
                "Table ref invalid for this database or insufficient permissions".to_string(),
            ));
        };
        statement.push_str(server.table_name.as_str());
        statement.push(' ');
        Ok(())
    })?;

    //TODO Probably some iterator based way to do this, clean up later
    statement.push_str(" (");
    for col_name in cols.iter() {
        statement.push_str(col_name?.to_str()?);
        statement.push_str(", ");
    }
    if statement.as_bytes()[statement.len() - 2] == b',' {
        statement.truncate(statement.len() - 2);
    }
    statement.push_str(") ");

    //TODO implement from rusqlite::Error for capnp::error and use ? instead of unwrap
    match source.which()? {
        sqlite_capnp::insert::source::Which::Values(values) => {
            statement.push_str("VALUES ");

            for value in values?.iter() {
                statement.push_str("(");
                for dbany in value?.iter() {
                    match dbany.which()? {
                        sqlite_capnp::d_b_any::Which::Null(_) => {
                            sql_params.push(rusqlite::types::Value::Null);
                            statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                        }
                        sqlite_capnp::d_b_any::Which::Integer(int) => {
                            sql_params.push(rusqlite::types::Value::Integer(int));
                            statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                        }
                        sqlite_capnp::d_b_any::Which::Real(real) => {
                            sql_params.push(rusqlite::types::Value::Real(real));
                            statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                        }
                        sqlite_capnp::d_b_any::Which::Text(text) => {
                            sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                            statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                        }
                        sqlite_capnp::d_b_any::Which::Blob(blob) => {
                            sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                            statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                        }
                        sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                            todo!()
                        }
                    }
                }
                if statement.as_bytes()[statement.len() - 2] == b',' {
                    statement.truncate(statement.len() - 2);
                }
                statement.push_str("), ");
            }
        }
        sqlite_capnp::insert::source::Which::Select(select) => {
            let select_and_params =
                build_select_statement(select?, statement, sql_params, bindparam_indexes)?;
            statement = select_and_params.statement;
            sql_params = select_and_params.params;
            bindparam_indexes = select_and_params.bindparam_indexes;
        }
        sqlite_capnp::insert::source::Which::Defaults(defaults) => {
            statement.push_str("DEFAULT VALUES");
        }
    }
    if statement.as_bytes()[statement.len() - 2] == b',' {
        statement.truncate(statement.len() - 2);
    }

    if let Some(_) = returning.iter().next() {
        statement.push_str(" RETURNING ")
    }
    for expr in returning.iter() {
        match expr.which()? {
            sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
                sqlite_capnp::d_b_any::Which::Null(_) => {
                    sql_params.push(rusqlite::types::Value::Null);
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Integer(int) => {
                    sql_params.push(rusqlite::types::Value::Integer(int));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Real(real) => {
                    sql_params.push(rusqlite::types::Value::Real(real));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Text(text) => {
                    sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Blob(blob) => {
                    sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                    todo!()
                }
            },
            sqlite_capnp::expr::Which::Bindparam(_) => {
                sql_params.push(rusqlite::types::Value::Null);
                bindparam_indexes.push(sql_params.len() - 1);
                statement.push_str(format!("?{}, ", sql_params.len()).as_str());
            }
            sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
            sqlite_capnp::expr::Which::Functioninvocation(func) => {
                let statement_and_params =
                    build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
                statement = statement_and_params.statement;
                sql_params = statement_and_params.params;
                bindparam_indexes = statement_and_params.bindparam_indexes;
            }
        }
    }

    if statement.as_bytes()[statement.len() - 2] == b',' {
        statement.truncate(statement.len() - 2);
    }
    return Ok(StatementAndParams {
        statement: statement,
        params: sql_params,
        bindparam_indexes: bindparam_indexes,
    });
}

fn build_delete_statement<'a>(
    del: delete::Reader<'a>,
    mut statement: String,
    mut sql_params: Vec<rusqlite::types::Value>,
    mut bindparam_indexes: Vec<usize>,
) -> eyre::Result<StatementAndParams> {
    capnp_let!({from, returning, r#where} = del);
    statement.push_str("DELETE FROM ");

    //let tableref = capnp::capability::get_resolved_cap(from).await;
    let tableref = from;
    TABLE_REF_SET.with_borrow_mut(|set| {
        let Some(server) = set.get_local_server_of_resolved(&tableref) else {
            return Err(capnp::Error::failed(
                "Table ref invalid for this database or insufficient permissions".to_string(),
            ));
        };
        statement.push_str(server.table_name.as_str());
        statement.push(' ');
        Ok(())
    })?;

    match r#where.which()? {
        sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
            sqlite_capnp::d_b_any::Which::Null(_) => {}
            sqlite_capnp::d_b_any::Which::Integer(int) => {
                sql_params.push(rusqlite::types::Value::Integer(int));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Real(real) => {
                sql_params.push(rusqlite::types::Value::Real(real));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Text(text) => {
                sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Blob(blob) => {
                sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                todo!()
            }
        },
        sqlite_capnp::expr::Which::Bindparam(_) => {
            sql_params.push(rusqlite::types::Value::Null);
            bindparam_indexes.push(sql_params.len() - 1);
            statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
        }
        sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
        sqlite_capnp::expr::Which::Functioninvocation(func) => {
            statement.push_str("WHERE ");
            let statement_and_params =
                build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
            statement = statement_and_params.statement;
            sql_params = statement_and_params.params;
            bindparam_indexes = statement_and_params.bindparam_indexes;
        }
    }

    if statement.as_bytes()[statement.len() - 2] == b',' {
        statement.truncate(statement.len() - 2);
    }

    if let Some(_returning_expr) = returning.iter().next() {
        statement.push_str(" RETURNING ")
    }
    for returning_expr in returning.iter() {
        match returning_expr.which()? {
            sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
                sqlite_capnp::d_b_any::Which::Null(_) => {
                    sql_params.push(rusqlite::types::Value::Null);
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Integer(int) => {
                    sql_params.push(rusqlite::types::Value::Integer(int));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Real(real) => {
                    sql_params.push(rusqlite::types::Value::Real(real));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Text(text) => {
                    sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Blob(blob) => {
                    sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                    todo!()
                }
            },
            sqlite_capnp::expr::Which::Bindparam(_) => {
                sql_params.push(rusqlite::types::Value::Null);
                bindparam_indexes.push(sql_params.len() - 1);
                statement.push_str(format!("?{}, ", sql_params.len()).as_str());
            }
            sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
            sqlite_capnp::expr::Which::Functioninvocation(func) => {
                let statement_and_params =
                    build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
                statement = statement_and_params.statement;
                sql_params = statement_and_params.params;
                bindparam_indexes = statement_and_params.bindparam_indexes;
            }
        }
    }
    if statement.as_bytes()[statement.len() - 2] == b',' {
        statement.truncate(statement.len() - 2);
    }
    return Ok(StatementAndParams {
        statement: statement,
        params: sql_params,
        bindparam_indexes: bindparam_indexes,
    });
}
fn build_update_statement<'a>(
    upd: update::Reader<'a>,
    mut statement: String,
    mut sql_params: Vec<rusqlite::types::Value>,
    mut bindparam_indexes: Vec<usize>,
) -> Result<StatementAndParams, capnp::Error> {
    capnp_let!({assignments, from, r#where, returning} = upd);

    statement.push_str("UPDATE OR ");
    match upd.get_fallback()? {
        sqlite_capnp::update::ConflictStrategy::Abort => statement.push_str("ABORT "),
        sqlite_capnp::update::ConflictStrategy::Fail => statement.push_str("FAIL "),
        sqlite_capnp::update::ConflictStrategy::Ignore => statement.push_str("IGNORE "),
        sqlite_capnp::update::ConflictStrategy::Rollback => statement.push_str("ROLLBACK "),
        sqlite_capnp::update::ConflictStrategy::Replace => statement.push_str("REPLACE "),
    }

    match from.reborrow().get_tableorsubquery()?.which()? {
        sqlite_capnp::table_or_subquery::Which::Tableref(tableref) => {
            //let tableref = capnp::capability::get_resolved_cap(tableref?).await;
            ROTABLE_REF_SET.with_borrow_mut(|set| {
                let Some(server) = set.get_local_server_of_resolved(&tableref?) else {
                    return Err(capnp::Error::failed(
                        "Table ref invalid for this database or insufficient permissions"
                            .to_string(),
                    ));
                };
                statement.push_str(server.table_name.as_str());
                Ok(())
            })?;
        }
        sqlite_capnp::table_or_subquery::Which::Tablefunctioninvocation(_) => todo!(),
        sqlite_capnp::table_or_subquery::Which::Select(select) => {
            let select_and_params =
                build_select_statement(select?, statement, sql_params, bindparam_indexes)?;
            statement = select_and_params.statement;
            sql_params = select_and_params.params;
            bindparam_indexes = select_and_params.bindparam_indexes;
        }
        sqlite_capnp::table_or_subquery::Which::Joinclause(_) => todo!(),
    }

    if let Some(_) = assignments.iter().next() {
        statement.push_str(" SET ");
    } else {
        return Err(capnp::Error::failed(
            "Must provide at least one assignment".to_string(),
        ));
    }
    for assignment in assignments.iter() {
        statement.push_str(assignment.get_name()?.to_str()?);
        statement.push_str(" = ");
        match assignment.get_expr()?.which()? {
            sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
                sqlite_capnp::d_b_any::Which::Null(_) => {
                    sql_params.push(rusqlite::types::Value::Null);
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Integer(int) => {
                    sql_params.push(rusqlite::types::Value::Integer(int));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Real(real) => {
                    sql_params.push(rusqlite::types::Value::Real(real));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Text(text) => {
                    sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Blob(blob) => {
                    sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                    todo!()
                }
            },
            sqlite_capnp::expr::Which::Bindparam(_) => {
                sql_params.push(rusqlite::types::Value::Null);
                bindparam_indexes.push(sql_params.len() - 1);
                statement.push_str(format!("?{}, ", sql_params.len()).as_str());
            }
            sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
            sqlite_capnp::expr::Which::Functioninvocation(func) => {
                let statement_and_params =
                    build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
                statement = statement_and_params.statement;
                sql_params = statement_and_params.params;
                bindparam_indexes = statement_and_params.bindparam_indexes;
            }
        }
    }
    if statement.as_bytes()[statement.len() - 2] == b',' {
        statement.truncate(statement.len() - 2);
    }

    //TODO join operations
    if let Some(_) = from.get_joinoperations().iter().next() {
        statement.push_str(" FROM ");
    }
    for op in from.get_joinoperations().iter() {}

    match r#where.which()? {
        sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
            sqlite_capnp::d_b_any::Which::Null(_) => {}
            sqlite_capnp::d_b_any::Which::Integer(int) => {
                sql_params.push(rusqlite::types::Value::Integer(int));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Real(real) => {
                sql_params.push(rusqlite::types::Value::Real(real));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Text(text) => {
                sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Blob(blob) => {
                sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                todo!()
            }
        },
        sqlite_capnp::expr::Which::Bindparam(_) => {
            sql_params.push(rusqlite::types::Value::Null);
            bindparam_indexes.push(sql_params.len() - 1);
            statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
        }
        sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
        sqlite_capnp::expr::Which::Functioninvocation(func) => {
            statement.push_str("WHERE ");
            let statement_and_params =
                build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
            statement = statement_and_params.statement;
            sql_params = statement_and_params.params;
            bindparam_indexes = statement_and_params.bindparam_indexes;
        }
    }

    if let Some(_returning_expr) = returning.iter().next() {
        statement.push_str(" RETURNING ")
    }
    for returning_expr in returning.iter() {
        match returning_expr.which()? {
            sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
                sqlite_capnp::d_b_any::Which::Null(_) => {
                    sql_params.push(rusqlite::types::Value::Null);
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Integer(int) => {
                    sql_params.push(rusqlite::types::Value::Integer(int));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Real(real) => {
                    sql_params.push(rusqlite::types::Value::Real(real));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Text(text) => {
                    sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Blob(blob) => {
                    sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                    todo!()
                }
            },
            sqlite_capnp::expr::Which::Bindparam(_) => {
                sql_params.push(rusqlite::types::Value::Null);
                bindparam_indexes.push(sql_params.len() - 1);
                statement.push_str(format!("?{}, ", sql_params.len()).as_str());
            }
            sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
            sqlite_capnp::expr::Which::Functioninvocation(func) => {
                let statement_and_params =
                    build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
                statement = statement_and_params.statement;
                sql_params = statement_and_params.params;
                bindparam_indexes = statement_and_params.bindparam_indexes;
            }
        }
    }
    if statement.as_bytes()[statement.len() - 2] == b',' {
        statement.truncate(statement.len() - 2);
    }
    return Ok(StatementAndParams {
        statement: statement,
        params: sql_params,
        bindparam_indexes: bindparam_indexes,
    });
}
fn build_select_statement<'a>(
    select: select::Reader<'a>,
    mut statement: String,
    mut sql_params: Vec<rusqlite::types::Value>,
    mut bindparam_indexes: Vec<usize>,
) -> Result<StatementAndParams, capnp::Error> {
    capnp_let!({names, selectcore : {from : {tableorsubquery, joinoperations}, result, r#where}, mergeoperations, orderby, limit} = select);
    statement.push_str("SELECT ");

    for expr in result.iter() {
        match expr.which()? {
            sqlite_capnp::expr::Which::Literal(dbany) => {
                //TODO apparently ?n params not supported in this part of select, so if these are even legal probably need some checks against injection
                match dbany?.which()? {
                    sqlite_capnp::d_b_any::Which::Null(_) => {
                        //sql_params.push(rusqlite::types::Value::Null);
                        //statement.push_str(format!("?{param_number}, ").as_str());
                        statement.push_str(format!("{}, ", "NULL").as_str());
                    }
                    sqlite_capnp::d_b_any::Which::Integer(int) => {
                        //sql_params.push(rusqlite::types::Value::Integer(int));
                        //statement.push_str(format!("?{param_number}, ").as_str());
                        statement.push_str(format!("{}, ", int).as_str());
                    }
                    sqlite_capnp::d_b_any::Which::Real(real) => {
                        //sql_params.push(rusqlite::types::Value::Real(real));
                        //statement.push_str(format!("?{param_number}, ").as_str());
                        statement.push_str(format!("{}, ", real).as_str());
                    }
                    sqlite_capnp::d_b_any::Which::Text(text) => {
                        //sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                        //statement.push_str(format!("?{param_number}, ").as_str());
                        statement.push_str(format!("{}, ", text?.to_str()?).as_str());
                    }
                    sqlite_capnp::d_b_any::Which::Blob(blob) => {
                        //sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                        //statement.push_str(format!("?{param_number}, ").as_str());
                        statement.push_str(format!("{}, ", std::str::from_utf8(blob?)?).as_str());
                    }
                    sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                        todo!()
                    }
                }
            }
            sqlite_capnp::expr::Which::Bindparam(_) => {
                //Can't be a bindparam
                //sql_params.push(rusqlite::types::Value::Null);
                //bindparam_indexes.push(sql_params.len() - 1);
                //statement.push_str(format!("?{param_number}, ").as_str());
            }
            sqlite_capnp::expr::Which::Tablereference(table_column) => {
                let table_column_reader = table_column?;
                //let name = table_column_reader.reborrow().get_col_name()?.to_str()?;
                //let index = table_column_reader.get_reference();
                //TODO incorrect
                /*statement.push_str(
                    format!(
                        "{}.{}",
                        table_column_reader.reborrow().get_col_name()?.to_str()?,
                        table_column_reader.get_reference()
                    )
                    .as_str(),
                );*/
            }
            sqlite_capnp::expr::Which::Functioninvocation(func) => {
                let statement_and_params =
                    build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
                statement = statement_and_params.statement;
                sql_params = statement_and_params.params;
                bindparam_indexes = statement_and_params.bindparam_indexes;
            }
        }
    }
    if statement.as_bytes()[statement.len() - 2] == b',' {
        statement.truncate(statement.len() - 2);
    }

    statement.push_str(" FROM ");
    match tableorsubquery.which()? {
        sqlite_capnp::table_or_subquery::Which::Tableref(tableref) => {
            //let tableref = capnp::capability::get_resolved_cap(tableref?).await;
            ROTABLE_REF_SET.with_borrow_mut(|set| {
                let Some(server) = set.get_local_server_of_resolved(&tableref?) else {
                    return Err(capnp::Error::failed(
                        "Table ref invalid for this database or insufficient permissions"
                            .to_string(),
                    ));
                };
                statement.push_str(server.table_name.as_str());
                statement.push(' ');
                Ok(())
            })?;
        }
        sqlite_capnp::table_or_subquery::Which::Tablefunctioninvocation(_) => todo!(),
        sqlite_capnp::table_or_subquery::Which::Select(select) => {
            let select_and_params =
                build_select_statement(select?, statement, sql_params, bindparam_indexes)?;
            statement = select_and_params.statement;
            sql_params = select_and_params.params;
            bindparam_indexes = select_and_params.bindparam_indexes;
        }
        sqlite_capnp::table_or_subquery::Which::Joinclause(_) => todo!(),
    }
    match r#where.which()? {
        sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
            sqlite_capnp::d_b_any::Which::Null(_) => {}
            sqlite_capnp::d_b_any::Which::Integer(int) => {
                sql_params.push(rusqlite::types::Value::Integer(int));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Real(real) => {
                sql_params.push(rusqlite::types::Value::Real(real));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Text(text) => {
                sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Blob(blob) => {
                sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                todo!()
            }
        },
        sqlite_capnp::expr::Which::Bindparam(_) => {
            sql_params.push(rusqlite::types::Value::Null);
            bindparam_indexes.push(sql_params.len() - 1);
            statement.push_str(format!("WHERE ?{}", sql_params.len()).as_str());
        }
        sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
        sqlite_capnp::expr::Which::Functioninvocation(func) => {
            statement.push_str("WHERE ");
            let statement_and_params =
                build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
            statement = statement_and_params.statement;
            sql_params = statement_and_params.params;
            bindparam_indexes = statement_and_params.bindparam_indexes;
        }
    }

    for merge_operation in mergeoperations.iter() {
        match merge_operation.get_operator()? {
            select::merge_operation::MergeOperator::Union => statement.push_str(" UNION "),
            select::merge_operation::MergeOperator::Unionall => statement.push_str(" UNION ALL "),
            select::merge_operation::MergeOperator::Intersect => statement.push_str(" INTERSECT "),
            select::merge_operation::MergeOperator::Except => statement.push_str(" EXCEPT "),
        }
        let statement_and_params = build_select_statement(select, statement, sql_params, bindparam_indexes)?;
        statement = statement_and_params.statement;
        sql_params = statement_and_params.params;
        bindparam_indexes = statement_and_params.bindparam_indexes;
    }
    if let Some(_) = orderby.iter().next() {
        statement.push_str(" ORDER BY ");
    }
    for term in orderby.iter() {
        match term.get_expr()?.which()? {
            sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
                sqlite_capnp::d_b_any::Which::Null(_) => {
                    sql_params.push(rusqlite::types::Value::Null);
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Integer(int) => {
                    sql_params.push(rusqlite::types::Value::Integer(int));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Real(real) => {
                    sql_params.push(rusqlite::types::Value::Real(real));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Text(text) => {
                    sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Blob(blob) => {
                    sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                    todo!()
                }
            },
            sqlite_capnp::expr::Which::Bindparam(_) => {
                sql_params.push(rusqlite::types::Value::Null);
                bindparam_indexes.push(sql_params.len() - 1);
                statement.push_str(format!("?{}, ", sql_params.len()).as_str());
            }
            sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
            sqlite_capnp::expr::Which::Functioninvocation(func) => {
                let statement_and_params =
                    build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
                statement = statement_and_params.statement;
                sql_params = statement_and_params.params;
                bindparam_indexes = statement_and_params.bindparam_indexes;
            }
        }
        match term.get_direction()? {
            select::ordering_term::AscDesc::Asc => statement.push_str(" ASC, "),
            select::ordering_term::AscDesc::Desc => statement.push_str(" DSC, "),
        }
    }
    if statement.as_bytes()[statement.len() - 2] == b',' {
        statement.truncate(statement.len() - 2);
    }
    match limit.get_limit()?.which()? {
        sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
            sqlite_capnp::d_b_any::Which::Null(_) => {}
            sqlite_capnp::d_b_any::Which::Integer(int) => {
                sql_params.push(rusqlite::types::Value::Integer(int));
                statement.push_str(format!("LIMIT ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Real(real) => {
                sql_params.push(rusqlite::types::Value::Real(real));
                statement.push_str(format!("LIMIT ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Text(text) => {
                sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                statement.push_str(format!("LIMIT ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Blob(blob) => {
                sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                statement.push_str(format!("LIMIT ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                todo!()
            }
        },
        sqlite_capnp::expr::Which::Bindparam(_) => {
            sql_params.push(rusqlite::types::Value::Null);
            bindparam_indexes.push(sql_params.len() - 1);
            statement.push_str(format!("LIMIT ?{}", sql_params.len()).as_str());
        }
        sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
        sqlite_capnp::expr::Which::Functioninvocation(func) => {
            statement.push_str("LIMIT ");
            let statement_and_params =
                build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
            statement = statement_and_params.statement;
            sql_params = statement_and_params.params;
            bindparam_indexes = statement_and_params.bindparam_indexes;
        }
    }
    match limit.get_offset()?.which()? {
        sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
            sqlite_capnp::d_b_any::Which::Null(_) => {}
            sqlite_capnp::d_b_any::Which::Integer(int) => {
                sql_params.push(rusqlite::types::Value::Integer(int));
                statement.push_str(format!("OFFSET ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Real(real) => {
                sql_params.push(rusqlite::types::Value::Real(real));
                statement.push_str(format!("OFFSET ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Text(text) => {
                sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                statement.push_str(format!("OFFSET ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Blob(blob) => {
                sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                statement.push_str(format!("OFFSET ?{}", sql_params.len()).as_str());
            }
            sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                todo!()
            }
        },
        sqlite_capnp::expr::Which::Bindparam(_) => {
            sql_params.push(rusqlite::types::Value::Null);
            bindparam_indexes.push(sql_params.len() - 1);
            statement.push_str(format!("OFFSET ?{}", sql_params.len()).as_str());
        }
        sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
        sqlite_capnp::expr::Which::Functioninvocation(func) => {
            statement.push_str("OFFSET ");
            let statement_and_params =
                build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
            statement = statement_and_params.statement;
            sql_params = statement_and_params.params;
            bindparam_indexes = statement_and_params.bindparam_indexes;
        }
    }

    return Ok(StatementAndParams {
        statement: statement,
        params: sql_params,
        bindparam_indexes: bindparam_indexes,
    });
}
fn build_function_invocation<'a>(
    function_reader: function_invocation::Reader<'a>,
    mut statement: String,
    mut sql_params: Vec<rusqlite::types::Value>,
    mut bindparam_indexes: Vec<usize>,
) -> Result<StatementAndParams, capnp::Error> {
    SQL_FUNCTION_SET.with_borrow(|set| {
        let Some(server) =
            set.get_local_server_of_resolved(&function_reader.reborrow().get_function()?)
        else {
            return Err(capnp::Error::failed("Sql function cap invalid".to_string()));
        };
        statement.push_str(server.function.as_str());
        Ok(())
    })?;
    if let Some(_) = function_reader.reborrow().get_params()?.iter().next() {
        statement.push_str(" (");
    }
    let mut params_iter = function_reader.reborrow().get_params()?.iter();
    while let Some(param) = params_iter.next() {
        match param.which()? {
            sqlite_capnp::expr::Which::Literal(dbany) => match dbany?.which()? {
                sqlite_capnp::d_b_any::Which::Null(_) => {
                    sql_params.push(rusqlite::types::Value::Null);
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Integer(int) => {
                    sql_params.push(rusqlite::types::Value::Integer(int));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Real(real) => {
                    sql_params.push(rusqlite::types::Value::Real(real));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Text(text) => {
                    sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Blob(blob) => {
                    sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
                    statement.push_str(format!("?{}, ", sql_params.len()).as_str());
                }
                sqlite_capnp::d_b_any::Which::Pointer(pointer) => {
                    todo!()
                }
            },
            sqlite_capnp::expr::Which::Bindparam(_) => {
                sql_params.push(rusqlite::types::Value::Null);
                bindparam_indexes.push(sql_params.len() - 1);
                statement.push_str(format!("?{}, ", sql_params.len()).as_str());
            }
            sqlite_capnp::expr::Which::Tablereference(_) => todo!(),
            sqlite_capnp::expr::Which::Functioninvocation(func) => {
                let statement_and_params =
                    build_function_invocation(func?, statement, sql_params, bindparam_indexes)?;
                statement = statement_and_params.statement;
                sql_params = statement_and_params.params;
                bindparam_indexes = statement_and_params.bindparam_indexes;
            }
        }
        if statement.as_bytes()[statement.len() - 2] == b',' {
            statement.truncate(statement.len() - 2);
        }
        if let Some(_) = function_reader.reborrow().get_params()?.iter().next() {
            statement.push_str(")");
        }
    }
    return Ok(StatementAndParams {
        statement: statement,
        params: sql_params,
        bindparam_indexes: bindparam_indexes,
    });
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test() -> eyre::Result<()> {
        todo!()
    }
}
