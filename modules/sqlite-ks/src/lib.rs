use std::borrow::BorrowMut;
use std::cell::Cell;
use std::fmt::format;
use std::{cell::RefCell, ops::AddAssign, path::Path, rc::Rc};

use crate::sqlite_capnp::{add_d_b, database, r_o_database};
use capnp::capability::FromClientHook;
use capnp_macros::{capnp_build, capnp_let, capnproto_rpc};
use capnp_rpc::CapabilityServerSet;
use rusqlite::{params, params_from_iter, types::ToSqlOutput, Connection, OpenFlags, Result, ToSql};
use sqlite_capnp::{where_expr, d_b_any, delete, function_invocation, insert::source, prepared_statement, r_a_table_ref, r_o_table_ref, result_stream, select, sql_function, table, table_ref, update};
use sqlite_capnp::{expr, index, indexed_column, insert, join_clause, select_core, table_field, table_function_ref, table_or_subquery};
use sturdyref_capnp::{restorer, saveable};

capnp_import::capnp_import!("sqlite.capnp", "../../core/schema/std/sturdyref.capnp");

enum dbany {
    None,
    int(i64),
    real(f64),
    str(String),
    blob(Vec<u8>),
    pointer(Vec<u8>),
}
fn get_restorer() -> restorer::Client {
    //get the restorer module cap from somewhere
    todo!()
}
//TODO make a real result stream
struct PlaceholderResults {
    buffer: Vec<Vec<dbany>>,
    last_id: Cell<usize>,
}
#[capnproto_rpc(result_stream)]
impl result_stream::Server for PlaceholderResults {
    async fn next(&self, size: u16) {
        let last_id = self.last_id.get();
        let mut builder = results.get().init_res();
        if last_id + size as usize >= self.buffer.len() {
            builder.set_finished(true);
        }
        let mut results_builder = builder.init_results(self.buffer.len() as u32);
        self.last_id.replace(last_id + size as usize);

        for i in last_id..self.buffer.len() {
            let mut dbany_builder = results_builder.reborrow().init(i as u32, self.buffer[i].len() as u32);
            for j in 0..self.buffer[i].len() {
                match &self.buffer[i][j] {
                    dbany::None => dbany_builder.reborrow().get(j as u32).set_null(()),
                    dbany::int(int) => dbany_builder.reborrow().get(j as u32).set_integer(int.clone()),
                    dbany::real(r) => dbany_builder.reborrow().get(j as u32).set_real(r.clone()),
                    dbany::str(str) => dbany_builder.reborrow().get(j as u32).set_text(str.as_str().into()),
                    dbany::blob(blob) => dbany_builder.reborrow().get(j as u32).set_blob(blob),
                    dbany::pointer(key) => {
                        let mut request = get_restorer().restore_request();
                        request.get().init_value().set_as(key.as_slice())?;
                        let response = request.send().promise.await?;
                        let restored = response.get()?.get_cap()?;
                        dbany_builder.reborrow().get(j as u32).init_pointer().set_as(restored)?;
                    }
                }
            }
        }

        Ok(())
    }
}

thread_local! {
    static ROTABLE_REF_SET: RefCell<CapabilityServerSet<TableRefImpl, r_o_table_ref::Client>> =
        RefCell::new(CapabilityServerSet::new());
    static RATABLE_REF_SET: RefCell<CapabilityServerSet<TableRefImpl, r_a_table_ref::Client>> =
        RefCell::new(CapabilityServerSet::new());
    static TABLE_REF_SET: RefCell<CapabilityServerSet<TableRefImpl, table_ref::Client>> =
        RefCell::new(CapabilityServerSet::new());
    static TABLE_SET: RefCell<CapabilityServerSet<TableRefImpl, table::Client>> =
        RefCell::new(CapabilityServerSet::new());
}
struct SqliteDatabase {
    connection: Connection,
    prepared_insert_set: RefCell<CapabilityServerSet<StatementAndParams, prepared_statement::Client<insert::Owned>>>,
    prepared_select_set: RefCell<CapabilityServerSet<StatementAndParams, prepared_statement::Client<select::Owned>>>,
    prepared_delete_set: RefCell<CapabilityServerSet<StatementAndParams, prepared_statement::Client<delete::Owned>>>,
    prepared_update_set: RefCell<CapabilityServerSet<StatementAndParams, prepared_statement::Client<update::Owned>>>,
    index_set: RefCell<CapabilityServerSet<IndexImpl, index::Client>>,
    sql_function_set: RefCell<CapabilityServerSet<SqlFunction, sql_function::Client>>,
    table_function_set: RefCell<CapabilityServerSet<TableFunction, table_function_ref::Client>>,
}
impl SqliteDatabase {
    pub fn new<P: AsRef<Path> + Clone>(path: P, flags: OpenFlags) -> eyre::Result<(add_d_b::Client, Connection)> {
        let connection = Connection::open_with_flags(path.clone(), flags.clone())?;
        let server = SqliteDatabase {
            connection: connection,
            prepared_insert_set: RefCell::new(CapabilityServerSet::new()),
            prepared_select_set: RefCell::new(CapabilityServerSet::new()),
            prepared_delete_set: RefCell::new(CapabilityServerSet::new()),
            prepared_update_set: RefCell::new(CapabilityServerSet::new()),
            index_set: RefCell::new(CapabilityServerSet::new()),
            sql_function_set: RefCell::new(CapabilityServerSet::new()),
            table_function_set: RefCell::new(CapabilityServerSet::new()),
        };
        let client: add_d_b::Client = capnp_rpc::new_client(server);
        let conn = Connection::open_with_flags(path, flags)?;
        return Ok((client, conn));
    }
}
#[capnproto_rpc(r_o_database)]
impl r_o_database::Server for SqliteDatabase {
    async fn select(&self, q: Select) {
        let statement_and_params = build_select_statement(self, q, StatementAndParams::new(80)).await?;

        let mut stmt = self.connection.prepare(statement_and_params.statement.as_str()).map_err(convert_rusqlite_error)?;
        println!("{}", statement_and_params.statement); //Debugging
        println!("{:?}", statement_and_params.sql_params);
        let mut rows = stmt.query(params_from_iter(statement_and_params.sql_params.iter())).map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results.get().set_res(capnp_rpc::new_client(PlaceholderResults {
            buffer: row_vec,
            last_id: Cell::new(0),
        }));

        Ok(())
    }

    async fn prepare_select(&self, q: Select) {
        let statement_and_params = build_select_statement(self, q, StatementAndParams::new(80)).await?;
        let client = self.prepared_select_set.borrow_mut().new_client(statement_and_params);
        results.get().set_stmt(client);
        Ok(())
    }
    async fn run_prepared_select(&self, stmt: PreparedStatement<Select>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let Some(statement_and_params) = self.prepared_select_set.borrow_mut().get_local_server_of_resolved(&resolved) else {
            return Err(capnp::Error::failed("Prepared statement doesn't exist, or was created on a different machine".to_string()));
        };

        let mut prepared = self.connection.prepare_cached(statement_and_params.statement.as_str()).map_err(convert_rusqlite_error)?;
        let mut params = statement_and_params.sql_params.clone();
        fill_in_bindparams(&statement_and_params.bindparam_indexes, &mut params, bindings).await?;
        let mut rows = prepared.query(params_from_iter(params.iter())).map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results.get().set_res(capnp_rpc::new_client(PlaceholderResults {
            buffer: row_vec,
            last_id: Cell::new(0),
        }));
        Ok(())
    }
}

#[capnproto_rpc(database)]
impl database::Server for SqliteDatabase {
    async fn insert(&self, ins: Insert) {
        let statement_and_params = build_insert_statement(self, ins, StatementAndParams::new(100)).await?;
        println!("{}", statement_and_params.statement);
        println!("{:?}", statement_and_params.sql_params);

        let mut stmt = self.connection.prepare(statement_and_params.statement.as_str()).map_err(convert_rusqlite_error)?;
        let mut rows = stmt.query(params_from_iter(statement_and_params.sql_params.iter())).map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results.get().set_res(capnp_rpc::new_client(PlaceholderResults {
            buffer: row_vec,
            last_id: Cell::new(0),
        }));
        Ok(())
    }
    async fn prepare_insert(&self, ins: Insert) {
        let statement_and_params = build_insert_statement(self, ins, StatementAndParams::new(100)).await?;
        let client = self.prepared_insert_set.borrow_mut().new_client(statement_and_params);
        results.get().set_stmt(client);
        Ok(())
    }

    async fn run_prepared_insert(&self, stmt: PreparedStatement<Insert>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let Some(statement_and_params) = self.prepared_insert_set.borrow_mut().get_local_server_of_resolved(&resolved) else {
            return Err(capnp::Error::failed("Prepared statement doesn't exist, or was created on a different machine".to_string()));
        };
        let mut prepared = self.connection.prepare_cached(statement_and_params.statement.as_str()).map_err(convert_rusqlite_error)?;
        let mut params = statement_and_params.sql_params.clone();
        fill_in_bindparams(&statement_and_params.bindparam_indexes, &mut params, bindings).await?;
        println!("{}", statement_and_params.statement);
        println!("{:?}", params);
        let mut rows = prepared.query(params_from_iter(params.iter())).map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results.get().set_res(capnp_rpc::new_client(PlaceholderResults {
            buffer: row_vec,
            last_id: Cell::new(0),
        }));
        Ok(())
    }
    async fn update(&self, upd: Update) {
        let statement_and_params = build_update_statement(self, upd, StatementAndParams::new(120)).await?;
        println!("{}", statement_and_params.statement);
        println!("{:?}", statement_and_params.sql_params);
        let mut stmt = self.connection.prepare(statement_and_params.statement.as_str()).map_err(convert_rusqlite_error)?;
        let mut rows = stmt.query(params_from_iter(statement_and_params.sql_params.iter())).map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results.get().set_res(capnp_rpc::new_client(PlaceholderResults {
            buffer: row_vec,
            last_id: Cell::new(0),
        }));
        Ok(())
    }
    async fn prepare_update(&self, upd: Update) {
        let statement_and_params = build_update_statement(self, upd, StatementAndParams::new(120)).await?;
        let client = self.prepared_update_set.borrow_mut().new_client(statement_and_params);
        results.get().set_stmt(client);
        Ok(())
    }
    async fn run_prepared_update(&self, stmt: PreparedStatement<Update>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let Some(statement_and_params) = self.prepared_update_set.borrow_mut().get_local_server_of_resolved(&resolved) else {
            return Err(capnp::Error::failed("Prepared statement doesn't exist, or was created on a different machine".to_string()));
        };
        let mut prepared = self.connection.prepare_cached(statement_and_params.statement.as_str()).map_err(convert_rusqlite_error)?;
        let mut params = statement_and_params.sql_params.clone();
        fill_in_bindparams(&statement_and_params.bindparam_indexes, &mut params, bindings).await?;
        let mut rows = prepared.query(params_from_iter(params.iter())).map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results.get().set_res(capnp_rpc::new_client(PlaceholderResults {
            buffer: row_vec,
            last_id: Cell::new(0),
        }));
        Ok(())
    }
    async fn delete(&self, del: Delete) {
        let statement_and_params = build_delete_statement(self, del, StatementAndParams::new(80)).await?;

        let mut stmt = self.connection.prepare(statement_and_params.statement.as_str()).map_err(convert_rusqlite_error)?;
        let mut rows = stmt.query(params_from_iter(statement_and_params.sql_params.iter())).map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results.get().set_res(capnp_rpc::new_client(PlaceholderResults {
            buffer: row_vec,
            last_id: Cell::new(0),
        }));
        Ok(())
    }
    async fn prepare_delete(&self, del: Delete) {
        let statement_and_params = build_delete_statement(self, del, StatementAndParams::new(80)).await?;
        let client = self.prepared_delete_set.borrow_mut().new_client(statement_and_params);
        results.get().set_stmt(client);
        Ok(())
    }
    async fn run_prepared_delete(&self, stmt: PreparedStatement<Delete>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let Some(statement_and_params) = self.prepared_delete_set.borrow_mut().get_local_server_of_resolved(&resolved) else {
            return Err(capnp::Error::failed("Prepared statement doesn't exist, or was created on a different machine".to_string()));
        };
        let mut prepared = self.connection.prepare_cached(statement_and_params.statement.as_str()).map_err(convert_rusqlite_error)?;
        let mut params = statement_and_params.sql_params.clone();
        fill_in_bindparams(&statement_and_params.bindparam_indexes, &mut params, bindings).await?;
        let mut rows = prepared.query(params_from_iter(params.iter())).map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results.get().set_res(capnp_rpc::new_client(PlaceholderResults {
            buffer: row_vec,
            last_id: Cell::new(0),
        }));
        Ok(())
    }
}
#[derive(Clone)]
struct TableRefImpl {
    table_name: Rc<String>,
}
#[capnproto_rpc(r_o_table_ref)]
impl r_o_table_ref::Server for TableRefImpl {}
#[capnproto_rpc(r_a_table_ref)]
impl r_a_table_ref::Server for TableRefImpl {
    async fn readonly(&self) {
        let client: r_o_table_ref::Client = ROTABLE_REF_SET.with_borrow_mut(|set| set.new_client(TableRefImpl { table_name: self.table_name.clone() }));
        results.get().set_res(client);
        Ok(())
    }
}
#[capnproto_rpc(table_ref)]
impl table_ref::Server for TableRefImpl {
    async fn appendonly(&self) {
        let client: r_a_table_ref::Client = RATABLE_REF_SET.with_borrow_mut(|set| set.new_client(TableRefImpl { table_name: self.table_name.clone() }));
        results.get().set_res(client);
        Ok(())
    }
}
#[capnproto_rpc(table)]
impl table::Server for TableRefImpl {
    async fn adminless(&self) {
        let client: table_ref::Client = TABLE_REF_SET.with_borrow_mut(|set| set.new_client(TableRefImpl { table_name: self.table_name.clone() }));
        results.get().set_res(client);
        Ok(())
    }
}
struct IndexImpl {
    name: Rc<String>,
}

#[capnproto_rpc(index)]
impl index::Server for IndexImpl {}

#[capnproto_rpc(add_d_b)]
impl add_d_b::Server for SqliteDatabase {
    async fn create_table(&self, def: List) {
        let table = generate_table_name();
        let mut statement = String::new();
        statement.push_str("CREATE TABLE ");
        statement.push_str(table.table_name.as_str());
        statement.push_str(" (");
        statement.push_str("id INTEGER PRIMARY KEY, ");

        for field in def.iter() {
            statement.push_str(field.get_name()?.to_str()?);

            match field.get_base_type()? {
                table_field::Type::Integer => statement.push_str(" INTEGER"),
                table_field::Type::Real => statement.push_str(" REAL"),
                table_field::Type::Text => statement.push_str(" TEXT"),
                table_field::Type::Blob => statement.push_str(" BLOB"),
                table_field::Type::Pointer => statement.push_str(" BLOB"),
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
        self.connection.execute(statement.as_str(), ()).map_err(convert_rusqlite_error)?;
        let table_client: table::Client = capnp_rpc::new_client(table);
        results.get().set_res(table_client);
        Ok(())
    }
    async fn create_view(&self, names: List<Text>, def: Select) {
        let mut statement = String::new();
        statement.push_str("CREATE VIEW ");
        let view_name = create_view_name();
        statement.push_str(view_name.table_name.as_str());
        statement.push_str(" ");

        if !names.is_empty() {
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
        statement.push_str("AS ");
        let statement_and_params = build_select_statement(self, def, StatementAndParams::new(80)).await?;
        statement.push_str(statement_and_params.statement.as_str());
        self.connection
            .execute(statement.as_str(), params_from_iter(statement_and_params.sql_params.iter()))
            .map_err(convert_rusqlite_error)?;
        let client = ROTABLE_REF_SET.with_borrow_mut(|set| set.borrow_mut().new_client(view_name));
        results.get().set_res(client);
        Ok(())
    }
    async fn create_restricted_table(&self, base: Table, restriction: List<TableRestriction>) {
        results.get();
        todo!()
    }
    async fn create_index(&self, base: TableRef, cols: List<IndexedColumn>) {
        let mut statement_and_params = StatementAndParams::new(80);
        statement_and_params.statement.push_str("CREATE INDEX ");
        let index_name = create_index_name();
        statement_and_params.statement.push_str(index_name.name.as_str());
        statement_and_params.statement.push_str(" ON ");
        let base = capnp::capability::get_resolved_cap(base).await;
        TABLE_REF_SET.with_borrow_mut(|set| {
            let Some(server) = set.get_local_server_of_resolved(&base) else {
                return Err(capnp::Error::failed("Table ref invalid for this database or insufficient permissions".to_string()));
            };
            statement_and_params.statement.push_str(server.table_name.as_str());
            statement_and_params.statement.push_str(format!(" AS tableref{} ", statement_and_params.tableref_number).as_str());
            statement_and_params.tableref_number += 1;
            Ok(())
        })?;
        statement_and_params.statement.push_str("(");
        for index_column in cols.iter() {
            match index_column.which()? {
                indexed_column::Which::Name(name) => {
                    statement_and_params.statement.push_str(name?.to_str()?);
                }
                indexed_column::Which::Expr(expr) => {
                    match_expr(self, expr?, &mut statement_and_params).await?;
                }
            }
            statement_and_params.statement.push_str(", ");
        }
        if statement_and_params.statement.as_bytes()[statement_and_params.statement.len() - 2] == b',' {
            statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
        }
        statement_and_params.statement.push_str(")");

        if params.get()?.has_sql_where() {
            statement_and_params.statement.push_str("WHERE ");
            match_where(self, params.get()?.get_sql_where()?, &mut statement_and_params).await?;
        }
        let mut stmt = self.connection.prepare(statement_and_params.statement.as_str()).map_err(convert_rusqlite_error)?;
        let mut rows = stmt.query(params_from_iter(statement_and_params.sql_params.iter())).map_err(convert_rusqlite_error)?;
        results.get().set_res(self.index_set.borrow_mut().new_client(index_name));
        Ok(())
    }
}
struct SqlFunction {
    function: Rc<String>,
}
struct TableFunction {
    function: Rc<String>,
}
impl table_function_ref::Server for TableFunction {}
impl sql_function::Server for SqlFunction {}
impl prepared_statement::Server<insert::Owned> for StatementAndParams {}
impl prepared_statement::Server<select::Owned> for StatementAndParams {}
impl prepared_statement::Server<delete::Owned> for StatementAndParams {}
impl prepared_statement::Server<update::Owned> for StatementAndParams {}

fn generate_table_name() -> TableRefImpl {
    let name = format!("table{}", rand::random::<u64>().to_string());
    TableRefImpl { table_name: Rc::new(name) }
}
fn create_index_name() -> IndexImpl {
    let name = format!("index{}", rand::random::<u64>().to_string());
    IndexImpl { name: Rc::new(name) }
}
fn create_view_name() -> TableRefImpl {
    let name = format!("view{}", rand::random::<u64>().to_string());
    TableRefImpl { table_name: Rc::new(name) }
}
struct StatementAndParams {
    statement: String,
    sql_params: Vec<rusqlite::types::Value>,
    bindparam_indexes: Vec<usize>,
    tableref_number: u16,
}
impl StatementAndParams {
    fn new(min_statement_size: usize) -> Self {
        Self {
            statement: String::with_capacity(min_statement_size),
            sql_params: Vec::new(),
            bindparam_indexes: Vec::new(),
            tableref_number: 0,
        }
    }
}

async fn build_insert_statement<'a>(db: &SqliteDatabase, ins: insert::Reader<'a>, mut statement_and_params: StatementAndParams) -> capnp::Result<StatementAndParams> {
    let fallback_reader = ins.get_fallback()?;
    capnp_let!({target, cols, returning} = ins);
    let source = ins.get_source();

    statement_and_params.statement.push_str("INSERT OR ");
    match fallback_reader {
        insert::ConflictStrategy::Abort => statement_and_params.statement.push_str("ABORT INTO "),
        insert::ConflictStrategy::Fail => statement_and_params.statement.push_str("FAIL INTO "),
        insert::ConflictStrategy::Ignore => statement_and_params.statement.push_str("IGNORE INTO "),
        insert::ConflictStrategy::Rollback => statement_and_params.statement.push_str("ROLLBACK INTO "),
    };
    let target = capnp::capability::get_resolved_cap(target).await;
    RATABLE_REF_SET.with_borrow(|set| {
        let Some(server) = set.get_local_server_of_resolved(&target) else {
            return Err(capnp::Error::failed("Table ref invalid for this database or insufficient permissions".to_string()));
        };
        statement_and_params.statement.push_str(server.table_name.as_str());
        statement_and_params.statement.push_str(format!(" AS tableref{} ", statement_and_params.tableref_number).as_str());
        statement_and_params.tableref_number += 1;
        Ok(())
    })?;

    statement_and_params.statement.push_str(" (");
    for col_name in cols.iter() {
        statement_and_params.statement.push_str(col_name?.to_str()?);
        statement_and_params.statement.push_str(", ");
    }
    if statement_and_params.statement.as_bytes()[statement_and_params.statement.len() - 2] == b',' {
        statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
    }
    statement_and_params.statement.push_str(") ");

    match source.which()? {
        insert::source::Which::Values(values) => {
            statement_and_params.statement.push_str("VALUES ");
            for value in values?.iter() {
                statement_and_params.statement.push_str("(");
                for dbany in value?.iter() {
                    match_dbany(dbany, &mut statement_and_params).await?;
                    statement_and_params.statement.push_str(", ");
                }
                statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
                statement_and_params.statement.push_str("), ");
            }
        }
        insert::source::Which::Select(select) => {
            statement_and_params = build_select_statement(db, select?, statement_and_params).await?;
        }
        insert::source::Which::Defaults(_) => {
            statement_and_params.statement.push_str("DEFAULT VALUES");
        }
    }
    if statement_and_params.statement.as_bytes()[statement_and_params.statement.len() - 2] == b',' {
        statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
    }

    if !returning.is_empty() {
        statement_and_params.statement.push_str(" RETURNING ");
        for expr in returning.iter() {
            match_expr(db, expr, &mut statement_and_params).await?;
            statement_and_params.statement.push_str(", ");
        }
        statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
    }
    return Ok(statement_and_params);
}

async fn build_delete_statement<'a>(db: &SqliteDatabase, del: delete::Reader<'a>, mut statement_and_params: StatementAndParams) -> capnp::Result<StatementAndParams> {
    capnp_let!({from, returning} = del);
    statement_and_params.statement.push_str("DELETE FROM ");

    let tableref = capnp::capability::get_resolved_cap(from).await;
    TABLE_REF_SET.with_borrow_mut(|set| {
        let Some(server) = set.get_local_server_of_resolved(&tableref) else {
            return Err(capnp::Error::failed("Table ref invalid for this database or insufficient permissions".to_string()));
        };
        statement_and_params.statement.push_str(server.table_name.as_str());
        statement_and_params.statement.push_str(format!(" AS tableref{} ", statement_and_params.tableref_number).as_str());
        statement_and_params.tableref_number += 1;
        Ok(())
    })?;

    if del.has_sql_where() {
        statement_and_params.statement.push_str("WHERE ");
        match_where(db, del.get_sql_where()?, &mut statement_and_params).await?;
    }

    if !returning.is_empty() {
        statement_and_params.statement.push_str(" RETURNING ");

        for returning_expr in returning.iter() {
            match_expr(db, returning_expr, &mut statement_and_params).await?;
            statement_and_params.statement.push_str(", ");
        }
        statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
    }
    return Ok(statement_and_params);
}
async fn build_update_statement<'a>(db: &SqliteDatabase, upd: update::Reader<'a>, mut statement_and_params: StatementAndParams) -> Result<StatementAndParams, capnp::Error> {
    capnp_let!({assignments, from, returning} = upd);

    statement_and_params.statement.push_str("UPDATE OR ");
    match upd.get_fallback()? {
        update::ConflictStrategy::Abort => statement_and_params.statement.push_str("ABORT "),
        update::ConflictStrategy::Fail => statement_and_params.statement.push_str("FAIL "),
        update::ConflictStrategy::Ignore => statement_and_params.statement.push_str("IGNORE "),
        update::ConflictStrategy::Rollback => statement_and_params.statement.push_str("ROLLBACK "),
        update::ConflictStrategy::Replace => statement_and_params.statement.push_str("REPLACE "),
    }

    match from.reborrow().get_tableorsubquery()?.which()? {
        table_or_subquery::Which::Tableref(tableref) => {
            let tableref = capnp::capability::get_resolved_cap(tableref?).await;
            ROTABLE_REF_SET.with_borrow_mut(|set| {
                let Some(server) = set.get_local_server_of_resolved(&tableref) else {
                    return Err(capnp::Error::failed("Table ref invalid for this database or insufficient permissions".to_string()));
                };
                statement_and_params.statement.push_str(server.table_name.as_str());
                statement_and_params.statement.push_str(format!(" AS tableref{} ", statement_and_params.tableref_number).as_str());
                statement_and_params.tableref_number += 1;
                Ok(())
            })?;
        }
        table_or_subquery::Which::Tablefunctioninvocation(func) => {
            let func = func?;
            let Some(server) = db.table_function_set.borrow().get_local_server_of_resolved(&func.get_functionref()?) else {
                return Err(capnp::Error::failed("Table function ref invalid for this table or database".to_string()));
            };
            statement_and_params.statement.push_str(server.function.as_str());
            statement_and_params.statement.push_str(" (");
            for expr in func.get_exprs()? {
                match_expr(db, expr, &mut statement_and_params).await?;
                statement_and_params.statement.push_str(", ");
            }
            statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
            statement_and_params.statement.push_str(") ");
        }
        table_or_subquery::Which::Select(select) => {
            statement_and_params = build_select_statement(db, select?, statement_and_params).await?;
        }
        table_or_subquery::Which::Joinclause(join) => {
            statement_and_params = build_join_clause(db, join?, statement_and_params).await?;
        }
        table_or_subquery::Which::Null(()) => (),
    }

    if assignments.is_empty() {
        return Err(capnp::Error::failed("Must provide at least one assignment".to_string()));
    } else {
        statement_and_params.statement.push_str(" SET ");
        for assignment in assignments.iter() {
            statement_and_params.statement.push_str(assignment.get_name()?.to_str()?);
            statement_and_params.statement.push_str(" = ");
            match_expr(db, assignment.get_expr()?, &mut statement_and_params).await?;
            statement_and_params.statement.push_str(", ");
        }
        statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
    }

    if from.has_joinoperations() {
        statement_and_params.statement.push_str(" FROM ");
        statement_and_params = build_join_clause(db, from, statement_and_params).await?;
    }

    if upd.has_sql_where() {
        statement_and_params.statement.push_str(" WHERE ");
        match_where(db, upd.get_sql_where()?, &mut statement_and_params).await?;
    }

    if !returning.is_empty() {
        statement_and_params.statement.push_str(" RETURNING ");
        for returning_expr in returning.iter() {
            match_expr(db, returning_expr, &mut statement_and_params).await?;
            statement_and_params.statement.push_str(", ");
        }
        statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
    }
    return Ok(statement_and_params);
}
async fn build_select_statement<'a>(db: &SqliteDatabase, select: select::Reader<'a>, mut statement_and_params: StatementAndParams) -> Result<StatementAndParams, capnp::Error> {
    capnp_let!({names, selectcore : {from, results}, mergeoperations, orderby, limit} = select);
    statement_and_params.statement.push_str("SELECT ");
    let mut names_iter = names.iter();
    for expr in results.iter() {
        match expr.which()? {
            expr::Which::Literal(dbany) => {
                //TODO Probably need some checks against injection
                match dbany?.which()? {
                    d_b_any::Which::Null(_) => {
                        statement_and_params.statement.push_str(format!("{}, ", "NULL").as_str());
                    }
                    d_b_any::Which::Integer(int) => {
                        statement_and_params.statement.push_str(format!("{}, ", int).as_str());
                    }
                    d_b_any::Which::Real(real) => {
                        statement_and_params.statement.push_str(format!("{}, ", real).as_str());
                    }
                    d_b_any::Which::Text(text) => {
                        statement_and_params.statement.push_str(format!("{}, ", text?.to_str()?).as_str());
                    }
                    d_b_any::Which::Blob(blob) => {
                        statement_and_params.statement.push_str(format!("{}, ", std::str::from_utf8(blob?)?).as_str());
                    }
                    d_b_any::Which::Pointer(pointer) => {
                        let response = pointer.get_as_capability::<saveable::Client>()?.save_request().send().promise.await?;
                        let restore_key = response.get()?.get_value().get_as::<&[u8]>()?;
                        statement_and_params.statement.push_str(format!("{}, ", std::str::from_utf8(restore_key)?).as_str());
                    }
                }
            }
            expr::Which::Bindparam(_) => {
                return Err(capnp::Error::failed("Select result can't be a bindparam".into()));
            }
            expr::Which::Tablereference(table_column) => {
                let table_column = table_column?;
                statement_and_params
                    .statement
                    .push_str(format!("tableref{}.{}, ", table_column.get_reference(), table_column.get_col_name()?.to_str()?).as_str());
            }
            expr::Which::Functioninvocation(func) => {
                build_function_invocation(db, func?, &mut statement_and_params).await?;
                statement_and_params.statement.push_str(", ");
            }
        }
        if let Some(name) = names_iter.next() {
            if statement_and_params.statement.as_bytes()[statement_and_params.statement.len() - 2] == b',' {
                statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
            }
            statement_and_params.statement.push_str(" AS ");
            statement_and_params.statement.push_str(name?.to_str()?);
            statement_and_params.statement.push_str(", ");
        }
    }
    if statement_and_params.statement.as_bytes()[statement_and_params.statement.len() - 2] == b',' {
        statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
    }

    if from.has_tableorsubquery() || from.has_joinoperations() {
        statement_and_params.statement.push_str(" FROM ");
        statement_and_params = Box::pin(build_join_clause(db, from, statement_and_params)).await?;
    }
    if selectcore.has_sql_where() {
        statement_and_params.statement.push_str("WHERE ");
        match_where(db, selectcore.get_sql_where()?, &mut statement_and_params).await?;
        statement_and_params.statement.push_str(" ");
    }

    for merge_operation in mergeoperations.iter() {
        match merge_operation.get_operator()? {
            select::merge_operation::MergeOperator::Union => statement_and_params.statement.push_str(" UNION "),
            select::merge_operation::MergeOperator::Unionall => statement_and_params.statement.push_str(" UNION ALL "),
            select::merge_operation::MergeOperator::Intersect => statement_and_params.statement.push_str(" INTERSECT "),
            select::merge_operation::MergeOperator::Except => statement_and_params.statement.push_str(" EXCEPT "),
        }
        statement_and_params = Box::pin(build_select_statement(db, select, statement_and_params)).await?;
    }
    if !orderby.is_empty() {
        statement_and_params.statement.push_str(" ORDER BY ");
        for term in orderby.iter() {
            match_expr(db, term.get_expr()?, &mut statement_and_params).await?;
            match term.get_direction()? {
                select::ordering_term::AscDesc::Asc => statement_and_params.statement.push_str(" ASC"),
                select::ordering_term::AscDesc::Desc => statement_and_params.statement.push_str(" DSC"),
            }
            statement_and_params.statement.push_str(", ");
        }
        statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
    }
    if limit.has_limit() {
        statement_and_params.statement.push_str("LIMIT ");
        match_expr(db, limit.get_limit()?, &mut statement_and_params).await?;
        statement_and_params.statement.push_str(" ");
    }
    if limit.has_offset() {
        statement_and_params.statement.push_str("OFFSET ");
        match_expr(db, limit.get_offset()?, &mut statement_and_params).await?;
        statement_and_params.statement.push_str(" ");
    }

    return Ok(statement_and_params);
}
async fn build_function_invocation<'a>(db: &SqliteDatabase, function_reader: function_invocation::Reader<'a>, statement_and_params: &mut StatementAndParams) -> Result<(), capnp::Error> {
    let Some(server) = db.sql_function_set.borrow().get_local_server_of_resolved(&function_reader.reborrow().get_function()?) else {
        return Err(capnp::Error::failed("Sql function cap invalid".to_string()));
    };
    statement_and_params.statement.push_str(server.function.as_str());
    if function_reader.has_params() {
        statement_and_params.statement.push_str(" (");
        for param in function_reader.get_params()?.iter() {
            Box::pin(match_expr(db, param, statement_and_params)).await?;
            statement_and_params.statement.push_str(", ");
        }
        statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
        statement_and_params.statement.push_str(")");
    }
    return Ok(());
}
async fn build_join_clause<'a>(db: &SqliteDatabase, join_clause: join_clause::Reader<'a>, mut statement_and_params: StatementAndParams) -> Result<StatementAndParams, capnp::Error> {
    match join_clause.get_tableorsubquery()?.which()? {
        table_or_subquery::Which::Tableref(tableref) => {
            let tableref = capnp::capability::get_resolved_cap(tableref?).await;
            ROTABLE_REF_SET.with_borrow_mut(|set| {
                let Some(server) = set.get_local_server_of_resolved(&tableref) else {
                    return Err(capnp::Error::failed("Table ref invalid for this database or insufficient permissions".to_string()));
                };
                statement_and_params.statement.push_str(server.table_name.as_str());
                statement_and_params.statement.push_str(format!(" AS tableref{} ", statement_and_params.tableref_number).as_str());
                statement_and_params.tableref_number += 1;
                Ok(())
            })?;
        }
        table_or_subquery::Which::Tablefunctioninvocation(func) => {
            let func = func?;
            let Some(server) = db.table_function_set.borrow().get_local_server_of_resolved(&func.get_functionref()?) else {
                return Err(capnp::Error::failed("Table function ref invalid for this table or database".to_string()));
            };
            statement_and_params.statement.push_str(server.function.as_str());
            statement_and_params.statement.push_str(" (");
            for expr in func.get_exprs()? {
                match_expr(db, expr, &mut statement_and_params).await?;
                statement_and_params.statement.push_str(", ");
            }
            statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
            statement_and_params.statement.push_str(") ");
        }
        table_or_subquery::Which::Select(select) => {
            statement_and_params = build_select_statement(db, select?, statement_and_params).await?;
        }
        table_or_subquery::Which::Joinclause(join) => {
            statement_and_params = Box::pin(build_join_clause(db, join?, statement_and_params)).await?;
        }
        table_or_subquery::Which::Null(()) => (),
    }
    for op in join_clause.get_joinoperations()?.iter() {
        match op.get_operator()?.which()? {
            join_clause::join_operation::join_operator::Which::InnerJoin(_) => {
                statement_and_params.statement.push_str("INNER JOIN ");
            }
            join_clause::join_operation::join_operator::Which::OuterJoin(p) => {
                match p? {
                    join_clause::join_operation::join_operator::JoinParameter::Left => statement_and_params.statement.push_str("LEFT OUTER "),
                    join_clause::join_operation::join_operator::JoinParameter::Right => statement_and_params.statement.push_str("RIGHT OUTER "),
                    join_clause::join_operation::join_operator::JoinParameter::Full => statement_and_params.statement.push_str("FULL OUTER "),
                    join_clause::join_operation::join_operator::JoinParameter::None => (),
                }
                statement_and_params.statement.push_str("JOIN ");
            }
            join_clause::join_operation::join_operator::Which::PlainJoin(p) => {
                match p? {
                    join_clause::join_operation::join_operator::JoinParameter::Left => statement_and_params.statement.push_str("LEFT "),
                    join_clause::join_operation::join_operator::JoinParameter::Right => statement_and_params.statement.push_str("RIGHT "),
                    join_clause::join_operation::join_operator::JoinParameter::Full => statement_and_params.statement.push_str("FULL "),
                    join_clause::join_operation::join_operator::JoinParameter::None => (),
                }
                statement_and_params.statement.push_str("JOIN ");
            }
        }
        match op.get_tableorsubquery()?.which()? {
            table_or_subquery::Which::Tableref(tableref) => {
                let tableref = capnp::capability::get_resolved_cap(tableref?).await;
                ROTABLE_REF_SET.with_borrow_mut(|set| {
                    let Some(server) = set.get_local_server_of_resolved(&tableref) else {
                        return Err(capnp::Error::failed("Table ref invalid for this database or insufficient permissions".to_string()));
                    };
                    statement_and_params.statement.push_str(server.table_name.as_str());
                    statement_and_params.statement.push_str(format!(" AS tableref{} ", statement_and_params.tableref_number).as_str());
                    statement_and_params.tableref_number += 1;
                    Ok(())
                })?;
            }
            table_or_subquery::Which::Tablefunctioninvocation(func) => {
                let func = func?;
                let Some(server) = db.table_function_set.borrow().get_local_server_of_resolved(&func.get_functionref()?) else {
                    return Err(capnp::Error::failed("Table function ref invalid for this table or database".to_string()));
                };
                statement_and_params.statement.push_str(server.function.as_str());
                statement_and_params.statement.push_str(" (");
                for expr in func.get_exprs()? {
                    match_expr(db, expr, &mut statement_and_params).await?;
                    statement_and_params.statement.push_str(", ");
                }
                statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
                statement_and_params.statement.push_str(") ");
            }
            table_or_subquery::Which::Select(select) => {
                statement_and_params = build_select_statement(db, select?, statement_and_params).await?;
            }
            table_or_subquery::Which::Joinclause(join) => {
                statement_and_params = Box::pin(build_join_clause(db, join?, statement_and_params)).await?;
            }
            table_or_subquery::Which::Null(()) => (),
        }
        match op.get_joinconstraint()?.which()? {
            join_clause::join_operation::join_constraint::Which::Expr(expr) => {
                statement_and_params.statement.push_str("ON ");
                match_expr(db, expr?, &mut statement_and_params).await?;
            }
            join_clause::join_operation::join_constraint::Which::Cols(cols) => {
                statement_and_params.statement.push_str("USING ");
                statement_and_params.statement.push_str("(");
                for col_name in cols?.iter() {
                    statement_and_params.statement.push_str(format!("{}, ", col_name?.to_str()?).as_str());
                }
                statement_and_params.statement.truncate(statement_and_params.statement.len() - 2);
                statement_and_params.statement.push_str(")");
            }
            join_clause::join_operation::join_constraint::Which::Empty(_) => (),
        }
    }
    return Ok(statement_and_params);
}
fn build_results_stream_buffer<'a>(mut rows: rusqlite::Rows<'a>) -> capnp::Result<Vec<Vec<dbany>>> {
    let mut row_vec = Vec::new();
    while let Ok(Some(row)) = rows.next() {
        let mut value_vec = Vec::new();
        let mut i = 0;
        while let Ok(value) = row.get_ref(i) {
            match value {
                rusqlite::types::ValueRef::Null => value_vec.push(dbany::None),
                rusqlite::types::ValueRef::Integer(int) => value_vec.push(dbany::int(int)),
                rusqlite::types::ValueRef::Real(r) => value_vec.push(dbany::real(r)),
                rusqlite::types::ValueRef::Text(t) => value_vec.push(dbany::str(std::str::from_utf8(t)?.to_string())),
                rusqlite::types::ValueRef::Blob(b) => value_vec.push(dbany::blob(b.to_vec())),
            }
            i += 1;
        }
        row_vec.push(value_vec)
    }
    return Ok(row_vec);
}
async fn match_expr<'a>(db: &SqliteDatabase, expr: expr::Reader<'a>, statement_and_params: &mut StatementAndParams) -> capnp::Result<()> {
    match expr.which()? {
        expr::Which::Literal(dbany) => {
            match_dbany(dbany?, statement_and_params).await?;
        }
        expr::Which::Bindparam(_) => {
            statement_and_params.sql_params.push(rusqlite::types::Value::Null);
            statement_and_params.bindparam_indexes.push(statement_and_params.sql_params.len() - 1);
            statement_and_params.statement.push_str(format!("?{}", statement_and_params.sql_params.len()).as_str());
        }
        expr::Which::Tablereference(table_column) => {
            let table_column = table_column?;
            statement_and_params
                .statement
                .push_str(format!("tableref{}.{}", table_column.get_reference(), table_column.get_col_name()?.to_str()?).as_str());
        }
        expr::Which::Functioninvocation(func) => {
            build_function_invocation(db, func?, statement_and_params).await?;
        }
    }
    Ok(())
}
async fn match_dbany<'a>(dbany: d_b_any::Reader<'a>, statement_and_params: &mut StatementAndParams) -> capnp::Result<()> {
    match dbany.which()? {
        d_b_any::Which::Null(_) => {
            statement_and_params.sql_params.push(rusqlite::types::Value::Null);
            statement_and_params.statement.push_str(format!("?{}", statement_and_params.sql_params.len()).as_str());
        }
        d_b_any::Which::Integer(int) => {
            statement_and_params.sql_params.push(rusqlite::types::Value::Integer(int));
            statement_and_params.statement.push_str(format!("?{}", statement_and_params.sql_params.len()).as_str());
        }
        d_b_any::Which::Real(real) => {
            statement_and_params.sql_params.push(rusqlite::types::Value::Real(real));
            statement_and_params.statement.push_str(format!("?{}", statement_and_params.sql_params.len()).as_str());
        }
        d_b_any::Which::Text(text) => {
            statement_and_params.sql_params.push(rusqlite::types::Value::Text(text?.to_string()?));
            statement_and_params.statement.push_str(format!("?{}", statement_and_params.sql_params.len()).as_str());
        }
        d_b_any::Which::Blob(blob) => {
            statement_and_params.sql_params.push(rusqlite::types::Value::Blob(blob?.to_vec()));
            statement_and_params.statement.push_str(format!("?{}", statement_and_params.sql_params.len()).as_str());
        }
        d_b_any::Which::Pointer(pointer) => {
            let response = pointer.get_as_capability::<saveable::Client>()?.save_request().send().promise.await?;
            let restore_key = response.get()?.get_value().get_as::<&[u8]>()?;
            statement_and_params.sql_params.push(rusqlite::types::Value::Blob(restore_key.to_vec()));
            statement_and_params.statement.push_str(format!("?{}", statement_and_params.sql_params.len()).as_str());
        }
    }
    Ok(())
}
async fn match_where<'a>(db: &SqliteDatabase, w_expr: where_expr::Reader<'a>, statement_and_params: &mut StatementAndParams) -> capnp::Result<()> {
    statement_and_params.statement.push_str(format!("{}", w_expr.get_column()?.to_str()?).as_str());
    if w_expr.get_operator_and_expr()?.is_empty() {
        return Err(capnp::Error::failed("Where clause is missing operator and condition".to_string()));
    }
    for w_expr in w_expr.get_operator_and_expr()?.iter() {
        match w_expr.get_operator()? {
            where_expr::Operator::Is => statement_and_params.statement.push_str(" IS "),
            where_expr::Operator::IsNot => statement_and_params.statement.push_str(" IS NOT "),
            where_expr::Operator::And => statement_and_params.statement.push_str(" AND "),
            where_expr::Operator::Or => statement_and_params.statement.push_str(" OR "),
        }
        if !w_expr.has_expr() {
            return Err(capnp::Error::failed("Where clause is missing condition".to_string()));
        }
        match_expr(db, w_expr.get_expr()?, statement_and_params).await?;
    }
    Ok(())
}
async fn fill_in_bindparams<'a>(bindparam_indexes: &Vec<usize>, params: &mut Vec<rusqlite::types::Value>, bindings_reader: capnp::struct_list::Reader<'a, d_b_any::Owned>) -> capnp::Result<()> {
    let mut bindings_iter = bindings_reader.iter();
    for index in bindparam_indexes {
        if let Some(param) = bindings_iter.next() {
            params[*index] = match param.which()? {
                d_b_any::Which::Null(_) => rusqlite::types::Value::Null,
                d_b_any::Which::Integer(i) => rusqlite::types::Value::Integer(i),
                d_b_any::Which::Real(r) => rusqlite::types::Value::Real(r),
                d_b_any::Which::Text(text) => rusqlite::types::Value::Text(text?.to_string()?),
                d_b_any::Which::Blob(blob) => rusqlite::types::Value::Blob(blob?.to_vec()),
                d_b_any::Which::Pointer(pointer) => {
                    let response = pointer.get_as_capability::<saveable::Client>()?.save_request().send().promise.await?;
                    let restore_key = response.get()?.get_value().get_as::<&[u8]>()?;
                    rusqlite::types::Value::Blob(restore_key.to_vec())
                }
            }
        } else {
            return Err(capnp::Error::failed("Not enough params provided for binding slots specified in prepare statement".to_string()));
        }
    }
    if let Some(_) = bindings_iter.next() {
        return Err(capnp::Error::failed("Too many params provided for binding slots specified in prepare statement".to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_test() -> eyre::Result<()> {
        let db_path = "test_db";
        let (client, connection) = SqliteDatabase::new(db_path, OpenFlags::default())?;

        let mut create_table_request = client.build_create_table_request(vec![
            table_field::TableField {
                _name: "name".to_string(),
                _base_type: table_field::Type::Text,
                _nullable: false,
            },
            table_field::TableField {
                _name: "data".to_string(),
                _base_type: table_field::Type::Blob,
                _nullable: true,
            },
        ]);

        let table_cap = create_table_request.send().promise.await?.get()?.get_res()?;

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

        let ro_tableref_cap = ra_table_ref_cap.readonly_request().send().promise.await?.get()?.get_res()?;

        let mut insert_request = client.clone().cast_to::<database::Client>().build_insert_request(Some(insert::Insert {
            _fallback: insert::ConflictStrategy::Fail,
            _target: ra_table_ref_cap.clone(),
            _source: source::Source::_Values(vec![vec![d_b_any::DBAny::_Text("Steven".to_string()), d_b_any::DBAny::_Null(())]]),
            _cols: vec!["name".to_string(), "data".to_string()],
            _returning: Vec::new(),
        }));
        insert_request.send().promise.await?;

        let mut insert_request = client.clone().cast_to::<database::Client>().build_insert_request(Some(insert::Insert {
            _fallback: insert::ConflictStrategy::Abort,
            _target: ra_table_ref_cap.clone(),
            _source: source::Source::_Values(vec![vec![d_b_any::DBAny::_Text("ToUpdate".to_string()), d_b_any::DBAny::_Blob(vec![4, 5, 6])]]),
            _cols: vec!["name".to_string(), "data".to_string()],
            _returning: Vec::new(),
        }));
        insert_request.send().promise.await?;

        let mut update_request = client.clone().cast_to::<database::Client>().build_update_request(Some(update::Update {
            _fallback: update::ConflictStrategy::Fail,
            _assignments: vec![
                update::assignment::Assignment {
                    _name: "name".to_string(),
                    _expr: Some(expr::Expr::_Literal(d_b_any::DBAny::_Text("Updated".to_string()))),
                },
                update::assignment::Assignment {
                    _name: "data".to_string(),
                    _expr: Some(expr::Expr::_Literal(d_b_any::DBAny::_Null(()))),
                },
            ],
            _from: Some(join_clause::JoinClause {
                _tableorsubquery: Some(table_or_subquery::TableOrSubquery::_Tableref(ro_tableref_cap.clone())),
                _joinoperations: Vec::new(),
            }),
            _sql_where: Some(where_expr::WhereExpr{_column: "name".to_string(), _operator_and_expr: vec![where_expr::op_and_expr::OpAndExpr{_operator: where_expr::Operator::Is, _expr: Some(expr::Expr::_Literal(d_b_any::DBAny::_Text("ToUpdate".to_string())))}]}),
            _returning: Vec::new(),
        }));
        update_request.send().promise.await?;

        let mut prepare_insert_request = client.clone().cast_to::<database::Client>().build_prepare_insert_request(Some(insert::Insert {
            _fallback: insert::ConflictStrategy::Ignore,
            _target: ra_table_ref_cap.clone(),
            _cols: vec!["name".to_string(), "data".to_string()],
            _source: insert::source::Source::_Values(vec![vec![d_b_any::DBAny::_Text("Mike".to_string()), d_b_any::DBAny::_Blob(vec![1, 2, 3])]]),
            _returning: vec![expr::Expr::_Bindparam(())],
        }));
        let prepared = prepare_insert_request.send().promise.await?.get()?.get_stmt()?;
        let mut run_request = client.clone().cast_to::<database::Client>().run_prepared_insert_request();
        run_request.get().set_stmt(prepared);
        run_request.get().init_bindings(1).get(0).set_text("meow".into());
        run_request.send().promise.await?;

        let mut select_request = client.clone().cast_to::<r_o_database::Client>().build_select_request(Some(select::Select {
            _selectcore: Some(Box::new(select_core::SelectCore {
                _from: Some(join_clause::JoinClause {
                    _tableorsubquery: Some(table_or_subquery::TableOrSubquery::_Tableref(ro_tableref_cap)),
                    _joinoperations: Vec::new(),
                }),
                _results: vec![
                    expr::Expr::_Literal(d_b_any::DBAny::_Text("id".to_string())),
                    expr::Expr::_Literal(d_b_any::DBAny::_Text("name".to_string())),
                    expr::Expr::_Literal(d_b_any::DBAny::_Text("data".to_string())),
                ],
                _sql_where: None,
            })),
            _mergeoperations: Vec::new(),
            _orderby: Vec::new(),
            _limit: None,
            _names: Vec::new(),
        }));

        let mut res_stream = select_request.send().promise.await?.get()?.get_res()?;
        let mut next_request = res_stream.next_request();
        next_request.get().set_size(8);
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
                    d_b_any::Which::Pointer(_) => print!("anypointer "),
                }
            }
            println!();
        }

        let mut delete_from_table_request = client.clone().cast_to::<database::Client>().delete_request();
        let mut builder = delete_from_table_request.get().init_del();
        let table_ref = table_cap.adminless_request().send().promise.await?.get()?.get_res()?;
        builder.set_from(table_ref);
        delete_from_table_request.send().promise.await?;

        Ok(())
    }
}

//TODO maybe needs to be more specific, but potentially would reveal information that should stay private
fn convert_rusqlite_error(err: rusqlite::Error) -> capnp::Error {
    match err {
        rusqlite::Error::SqliteFailure(_, _) => capnp::Error::failed(format!("Error from underlying sqlite call")),
        rusqlite::Error::SqliteSingleThreadedMode => capnp::Error::failed(format!("Attempting to open multiple connection when sqlite is in single threaded mode")),
        rusqlite::Error::FromSqlConversionFailure(_, _, _) => capnp::Error::failed(format!("Error converting sql type to rust type")),
        rusqlite::Error::IntegralValueOutOfRange(_, _) => capnp::Error::failed(format!("Integral value out of range")),
        rusqlite::Error::Utf8Error(e) => capnp::Error::from_kind(capnp::ErrorKind::TextContainsNonUtf8Data(e)),
        rusqlite::Error::NulError(_) => capnp::Error::failed(format!("Error converting string to c-compatible strting, because it contains an embeded null")),
        rusqlite::Error::InvalidParameterName(_) => capnp::Error::failed(format!("Invalid parameter name")),
        rusqlite::Error::InvalidPath(_) => capnp::Error::failed(format!("Invalid path")),
        rusqlite::Error::ExecuteReturnedResults => capnp::Error::failed(format!("Execute call returned rows")),
        rusqlite::Error::QueryReturnedNoRows => capnp::Error::failed(format!("Query that was expected to return rows returned no rows")),
        rusqlite::Error::InvalidColumnIndex(_) => capnp::Error::failed(format!("Invalid column index")),
        rusqlite::Error::InvalidColumnName(_) => capnp::Error::failed(format!("Invalid column name")),
        rusqlite::Error::InvalidColumnType(_, _, _) => capnp::Error::failed(format!("Invalid column type")),
        rusqlite::Error::StatementChangedRows(_) => capnp::Error::failed(format!("Query changed more/less rows than expected")),
        rusqlite::Error::ToSqlConversionFailure(_) => capnp::Error::failed(format!("Failed to convert type to sql type")),
        rusqlite::Error::InvalidQuery => capnp::Error::failed(format!("Invalid query")),
        rusqlite::Error::MultipleStatement => capnp::Error::failed(format!("Sql contains multiple statements")),
        rusqlite::Error::InvalidParameterCount(_, _) => capnp::Error::failed(format!("Invalid parameter count")),
        _ => capnp::Error::failed(format!("Sqlite error")),
    }
}
