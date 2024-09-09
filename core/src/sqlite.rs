use crate::buffer_allocator::BufferAllocator;
use crate::database::DatabaseExt;
use crate::keystone::CapabilityServerSetExt;
use crate::sqlite_capnp::root::ServerDispatch;
use crate::sqlite_capnp::{
    add_d_b, d_b_any, database, delete, expr, function_invocation, index, indexed_column, insert,
    insert::source, join_clause, prepared_statement, r_a_table_ref, r_o_database, r_o_table_ref,
    result_stream, select, select_core, sql_function, table, table_field, table_function_ref,
    table_or_subquery, table_ref, update, where_expr,
};
use crate::storage_capnp::{saveable, sturdy_ref};
use crate::sturdyref::SturdyRefImpl;
use capnp::capability::FromServer;
use capnp_macros::{capnp_let, capnproto_rpc};
use capnp_rpc::CapabilityServerSet;
use d_b_any::DBAny;
use rusqlite::{params_from_iter, Connection, OpenFlags, Result};
use std::borrow::{Borrow, BorrowMut};
use std::cell::Cell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::{cell::RefCell, path::Path, rc::Rc, rc::Weak};

enum SqlDBAny {
    None,
    Int(i64),
    Real(f64),
    Str(String),
    Blob(Vec<u8>),
    Pointer(i64),
}

//TODO make a real result stream
struct PlaceholderResults {
    buffer: Vec<Vec<SqlDBAny>>,
    last_id: Cell<usize>,
    db: Rc<ServerDispatch<SqliteDatabase>>,
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
            let mut dbany_builder = results_builder
                .reborrow()
                .init(i as u32, self.buffer[i].len() as u32);
            for j in 0..self.buffer[i].len() {
                match &self.buffer[i][j] {
                    SqlDBAny::None => dbany_builder.reborrow().get(j as u32).set_null(()),
                    SqlDBAny::Int(int) => dbany_builder.reborrow().get(j as u32).set_integer(*int),
                    SqlDBAny::Real(r) => dbany_builder.reborrow().get(j as u32).set_real(*r),
                    SqlDBAny::Str(str) => dbany_builder
                        .reborrow()
                        .get(j as u32)
                        .set_text(str.as_str().into()),
                    SqlDBAny::Blob(blob) => dbany_builder.reborrow().get(j as u32).set_blob(blob),
                    SqlDBAny::Pointer(key) => {
                        let client: sturdy_ref::Client<capnp::any_pointer::Owned> =
                            capnp_rpc::new_client(SturdyRefImpl::new(*key, self.db.clone()));
                        let response = client.restore_request().send().promise.await?;
                        let restored = response.get()?.get_cap()?;
                        dbany_builder
                            .reborrow()
                            .get(j as u32)
                            .init_pointer()
                            .set_as(restored)?;
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Default)]
pub struct DBCapSet {
    ro_table_ref_set: CapabilityServerSet<TableRefImpl, r_o_table_ref::Client>,
    ra_table_ref_set: CapabilityServerSet<TableRefImpl, r_a_table_ref::Client>,
    table_ref_set: CapabilityServerSet<TableRefImpl, table_ref::Client>,
    //table_set: CapabilityServerSet<TableRefImpl, table::Client>,
}

pub struct SqliteDatabase {
    pub connection: Connection,
    pub alloc: RefCell<BufferAllocator>,
    column_set: RefCell<HashSet<String>>,
    prepared_insert_set:
        RefCell<CapabilityServerSet<StatementAndParams, prepared_statement::Client<insert::Owned>>>,
    prepared_select_set:
        RefCell<CapabilityServerSet<StatementAndParams, prepared_statement::Client<select::Owned>>>,
    prepared_delete_set:
        RefCell<CapabilityServerSet<StatementAndParams, prepared_statement::Client<delete::Owned>>>,
    prepared_update_set:
        RefCell<CapabilityServerSet<StatementAndParams, prepared_statement::Client<update::Owned>>>,
    index_set: RefCell<CapabilityServerSet<IndexImpl, index::Client>>,
    sql_function_set: RefCell<CapabilityServerSet<SqlFunction, sql_function::Client>>,
    table_function_set: RefCell<CapabilityServerSet<TableFunction, table_function_ref::Client>>,
    sturdyref_set:
        RefCell<CapabilityServerSet<SturdyRefImpl, sturdy_ref::Client<capnp::any_pointer::Owned>>>,
    capset: Rc<RefCell<DBCapSet>>,
    pub clients: Rc<RefCell<HashMap<u64, Box<dyn capnp::private::capability::ClientHook>>>>,
    this: Weak<ServerDispatch<SqliteDatabase>>,
}

impl SqliteDatabase {
    pub fn new<P: AsRef<Path>>(
        path: P,
        flags: OpenFlags,
        capset: Rc<RefCell<DBCapSet>>,
        clients: Rc<RefCell<HashMap<u64, Box<dyn capnp::private::capability::ClientHook>>>>,
    ) -> rusqlite::Result<Rc<ServerDispatch<Self>>> {
        let connection = Connection::open_with_flags(path, flags)?;
        let result = Rc::new_cyclic(|this| {
            crate::sqlite_capnp::root::Client::from_server(Self {
                connection,
                column_set: Default::default(),
                alloc: Default::default(),
                prepared_insert_set: Default::default(),
                prepared_select_set: Default::default(),
                prepared_delete_set: Default::default(),
                prepared_update_set: Default::default(),
                index_set: Default::default(),
                sql_function_set: Default::default(),
                table_function_set: Default::default(),
                sturdyref_set: Default::default(),
                capset,
                clients,
                this: this.clone(),
            })
        });

        Ok(result)
    }

    pub fn new_connection(conn: Connection) -> Rc<ServerDispatch<Self>> {
        Rc::new_cyclic(|this| {
            crate::sqlite_capnp::root::Client::from_server(Self {
                connection: conn,
                alloc: Default::default(),
                column_set: Default::default(),
                prepared_insert_set: Default::default(),
                prepared_select_set: Default::default(),
                prepared_delete_set: Default::default(),
                prepared_update_set: Default::default(),
                index_set: Default::default(),
                sql_function_set: Default::default(),
                table_function_set: Default::default(),
                sturdyref_set: Default::default(),
                capset: Default::default(),
                clients: Default::default(),
                this: this.clone(),
            })
        })
    }
}

use crate::storage_capnp::restore;
const R_O_TABLE_REF: u8 = 1;
const R_A_TABLE_REF: u8 = 2;
const TABLE_REF: u8 = 3;
const INDEX_REF: u8 = 4;

impl restore::Server<crate::sqlite_capnp::storage::Owned> for SqliteDatabase {
    async fn restore(
        &self,
        params: restore::RestoreParams<crate::sqlite_capnp::storage::Owned>,
        mut results: restore::RestoreResults<crate::sqlite_capnp::storage::Owned>,
    ) -> Result<(), ::capnp::Error> {
        let data = params.get()?.get_data()?;
        let name = data.get_data()?.to_string()?;
        let mut capset = self.capset.as_ref().borrow_mut();
        let cap = match data.get_id() {
            R_O_TABLE_REF => {
                capset
                    .ro_table_ref_set
                    .new_client(TableRefImpl {
                        table_name: Rc::new(name),
                        db: self
                            .this
                            .upgrade()
                            .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
                    })
                    .client
                    .hook
            }
            R_A_TABLE_REF => {
                capset
                    .ra_table_ref_set
                    .new_client(TableRefImpl {
                        table_name: Rc::new(name),
                        db: self
                            .this
                            .upgrade()
                            .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
                    })
                    .client
                    .hook
            }
            TABLE_REF => {
                capset
                    .table_ref_set
                    .new_client(TableRefImpl {
                        table_name: Rc::new(name),
                        db: self
                            .this
                            .upgrade()
                            .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
                    })
                    .client
                    .hook
            }
            INDEX_REF => {
                self.index_set
                    .borrow_mut()
                    .new_client(IndexImpl {
                        name: Rc::new(name),
                        db: self
                            .this
                            .upgrade()
                            .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
                    })
                    .client
                    .hook
            }
            _ => return Err(capnp::Error::failed("unknown capability type!".into())),
        };

        results.get().init_cap().set_as_capability(cap);
        Ok(())
    }
}

impl crate::sqlite_capnp::root::Server for SqliteDatabase {}

#[capnproto_rpc(r_o_database)]
impl r_o_database::Server for SqliteDatabase {
    async fn select(&self, q: Select) {
        let statement_and_params =
            build_select_statement(self, q, StatementAndParams::new(80)).await?;

        let mut stmt = self
            .connection
            .prepare(statement_and_params.statement.as_str())
            .map_err(convert_rusqlite_error)?;
        let rows = stmt
            .query(params_from_iter(statement_and_params.sql_params.iter()))
            .map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results
            .get()
            .set_res(capnp_rpc::new_client(PlaceholderResults {
                buffer: row_vec,
                last_id: Cell::new(0),
                db: self
                    .this
                    .upgrade()
                    .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
            }));

        Ok(())
    }

    async fn prepare_select(&self, q: Select) {
        let statement_and_params =
            build_select_statement(self, q, StatementAndParams::new(80)).await?;
        let client = self
            .prepared_select_set
            .borrow_mut()
            .new_client(statement_and_params);
        results.get().set_stmt(client);
        Ok(())
    }
    async fn run_prepared_select(&self, stmt: PreparedStatement<Select>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let Some(statement_and_params) = self
            .prepared_select_set
            .borrow_mut()
            .get_local_server_of_resolved(&resolved)
        else {
            return Err(capnp::Error::failed(
                "Prepared statement doesn't exist, or was created on a different machine"
                    .to_string(),
            ));
        };

        let mut prepared = self
            .connection
            .prepare_cached(statement_and_params.statement.as_str())
            .map_err(convert_rusqlite_error)?;
        let mut params = statement_and_params.sql_params.clone();
        fill_in_bindparams(
            self,
            &statement_and_params.bindparam_indexes,
            &mut params,
            bindings,
        )
        .await?;
        let rows = prepared
            .query(params_from_iter(params.iter()))
            .map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results
            .get()
            .set_res(capnp_rpc::new_client(PlaceholderResults {
                buffer: row_vec,
                last_id: Cell::new(0),
                db: self
                    .this
                    .upgrade()
                    .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
            }));
        Ok(())
    }
}

#[capnproto_rpc(database)]
impl database::Server for SqliteDatabase {
    async fn insert(&self, ins: Insert) {
        let statement_and_params =
            build_insert_statement(self, ins, StatementAndParams::new(100)).await?;
        let mut stmt = self
            .connection
            .prepare(statement_and_params.statement.as_str())
            .map_err(convert_rusqlite_error)?;
        let rows = stmt
            .query(params_from_iter(statement_and_params.sql_params.iter()))
            .map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results
            .get()
            .set_res(capnp_rpc::new_client(PlaceholderResults {
                buffer: row_vec,
                last_id: Cell::new(0),
                db: self
                    .this
                    .upgrade()
                    .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
            }));
        Ok(())
    }
    async fn prepare_insert(&self, ins: Insert) {
        let statement_and_params =
            build_insert_statement(self, ins, StatementAndParams::new(100)).await?;
        let client = self
            .prepared_insert_set
            .borrow_mut()
            .new_client(statement_and_params);
        results.get().set_stmt(client);
        Ok(())
    }

    async fn run_prepared_insert(&self, stmt: PreparedStatement<Insert>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let Some(statement_and_params) = self
            .prepared_insert_set
            .borrow_mut()
            .get_local_server_of_resolved(&resolved)
        else {
            return Err(capnp::Error::failed(
                "Prepared statement doesn't exist, or was created on a different machine"
                    .to_string(),
            ));
        };
        let mut prepared = self
            .connection
            .prepare_cached(statement_and_params.statement.as_str())
            .map_err(convert_rusqlite_error)?;
        let mut params = statement_and_params.sql_params.clone();
        fill_in_bindparams(
            self,
            &statement_and_params.bindparam_indexes,
            &mut params,
            bindings,
        )
        .await?;
        let rows = prepared
            .query(params_from_iter(params.iter()))
            .map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results
            .get()
            .set_res(capnp_rpc::new_client(PlaceholderResults {
                buffer: row_vec,
                last_id: Cell::new(0),
                db: self
                    .this
                    .upgrade()
                    .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
            }));
        Ok(())
    }
    async fn update(&self, upd: Update) {
        let statement_and_params =
            build_update_statement(self, upd, StatementAndParams::new(120)).await?;
        let mut stmt = self
            .connection
            .prepare(statement_and_params.statement.as_str())
            .map_err(convert_rusqlite_error)?;
        let rows = stmt
            .query(params_from_iter(statement_and_params.sql_params.iter()))
            .map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results
            .get()
            .set_res(capnp_rpc::new_client(PlaceholderResults {
                buffer: row_vec,
                last_id: Cell::new(0),
                db: self
                    .this
                    .upgrade()
                    .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
            }));
        Ok(())
    }
    async fn prepare_update(&self, upd: Update) {
        let statement_and_params =
            build_update_statement(self, upd, StatementAndParams::new(120)).await?;
        let client = self
            .prepared_update_set
            .borrow_mut()
            .new_client(statement_and_params);
        results.get().set_stmt(client);
        Ok(())
    }
    async fn run_prepared_update(&self, stmt: PreparedStatement<Update>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let Some(statement_and_params) = self
            .prepared_update_set
            .borrow_mut()
            .get_local_server_of_resolved(&resolved)
        else {
            return Err(capnp::Error::failed(
                "Prepared statement doesn't exist, or was created on a different machine"
                    .to_string(),
            ));
        };
        let mut prepared = self
            .connection
            .prepare_cached(statement_and_params.statement.as_str())
            .map_err(convert_rusqlite_error)?;
        let mut params = statement_and_params.sql_params.clone();
        fill_in_bindparams(
            self,
            &statement_and_params.bindparam_indexes,
            &mut params,
            bindings,
        )
        .await?;
        let rows = prepared
            .query(params_from_iter(params.iter()))
            .map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results
            .get()
            .set_res(capnp_rpc::new_client(PlaceholderResults {
                buffer: row_vec,
                last_id: Cell::new(0),
                db: self
                    .this
                    .upgrade()
                    .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
            }));
        Ok(())
    }
    async fn delete(&self, del: Delete) {
        let statement_and_params =
            build_delete_statement(self, del, StatementAndParams::new(80)).await?;
        let mut stmt = self
            .connection
            .prepare(statement_and_params.statement.as_str())
            .map_err(convert_rusqlite_error)?;
        let rows = stmt
            .query(params_from_iter(statement_and_params.sql_params.iter()))
            .map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results
            .get()
            .set_res(capnp_rpc::new_client(PlaceholderResults {
                buffer: row_vec,
                last_id: Cell::new(0),
                db: self
                    .this
                    .upgrade()
                    .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
            }));
        Ok(())
    }
    async fn prepare_delete(&self, del: Delete) {
        let statement_and_params =
            build_delete_statement(self, del, StatementAndParams::new(80)).await?;
        let client = self
            .prepared_delete_set
            .borrow_mut()
            .new_client(statement_and_params);
        results.get().set_stmt(client);
        Ok(())
    }
    async fn run_prepared_delete(&self, stmt: PreparedStatement<Delete>, bindings: List<DBAny>) {
        let resolved = capnp::capability::get_resolved_cap(stmt).await;
        let Some(statement_and_params) = self
            .prepared_delete_set
            .borrow_mut()
            .get_local_server_of_resolved(&resolved)
        else {
            return Err(capnp::Error::failed(
                "Prepared statement doesn't exist, or was created on a different machine"
                    .to_string(),
            ));
        };
        let mut prepared = self
            .connection
            .prepare_cached(statement_and_params.statement.as_str())
            .map_err(convert_rusqlite_error)?;
        let mut params = statement_and_params.sql_params.clone();
        fill_in_bindparams(
            self,
            &statement_and_params.bindparam_indexes,
            &mut params,
            bindings,
        )
        .await?;
        let rows = prepared
            .query(params_from_iter(params.iter()))
            .map_err(convert_rusqlite_error)?;
        let row_vec = build_results_stream_buffer(rows)?;
        results
            .get()
            .set_res(capnp_rpc::new_client(PlaceholderResults {
                buffer: row_vec,
                last_id: Cell::new(0),
                db: self
                    .this
                    .upgrade()
                    .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
            }));
        Ok(())
    }
}
#[derive(Clone)]
struct TableRefImpl {
    table_name: Rc<String>,
    db: Rc<ServerDispatch<SqliteDatabase>>,
}

impl TableRefImpl {
    fn save_generic<T: capnp::traits::Owned>(
        &self,
        index: u8,
    ) -> capnp::Result<sturdy_ref::Client<T>> {
        let id = self
            .db
            .get_string_index(crate::keystone::BUILTIN_SQLITE)
            .map_err(|e| capnp::Error::failed(e.to_string()))?;
        let mut msg = capnp::message::Builder::new_default();
        let mut builder = msg.init_root::<crate::sqlite_capnp::storage::Builder>();
        builder.set_id(index);
        builder.set_data(self.table_name.as_ref().as_str().into());
        let cap: sturdy_ref::Client<T> = self.db.sturdyref_set.borrow_mut().new_generic_client(
            crate::sturdyref::SturdyRefImpl::init(
                id as u64,
                builder.into_reader(),
                self.db.clone(),
            )
            .map_err(|e| capnp::Error::failed(e.to_string()))?,
        );
        Ok(cap)
    }
}

#[capnproto_rpc(saveable)]
impl saveable::Server<r_o_table_ref::Owned> for TableRefImpl {
    async fn save(&self) {
        results
            .get()
            .set_ref(self.save_generic::<r_o_table_ref::Owned>(R_O_TABLE_REF)?);
        Ok(())
    }
}

#[capnproto_rpc(saveable)]
impl saveable::Server<r_a_table_ref::Owned> for TableRefImpl {
    async fn save(&self) {
        results
            .get()
            .set_ref(self.save_generic::<r_a_table_ref::Owned>(R_A_TABLE_REF)?);
        Ok(())
    }
}

#[capnproto_rpc(saveable)]
impl saveable::Server<table_ref::Owned> for TableRefImpl {
    async fn save(&self) {
        results
            .get()
            .set_ref(self.save_generic::<table_ref::Owned>(TABLE_REF)?);
        Ok(())
    }
}

#[capnproto_rpc(r_o_table_ref)]
impl r_o_table_ref::Server for TableRefImpl {}

#[capnproto_rpc(r_a_table_ref)]
impl r_a_table_ref::Server for TableRefImpl {
    async fn readonly(&self) {
        let client: r_o_table_ref::Client = self
            .db
            .server
            .capset
            .as_ref()
            .borrow_mut()
            .ro_table_ref_set
            .new_client(TableRefImpl {
                table_name: self.table_name.clone(),
                db: self.db.clone(),
            });
        results.get().set_res(client);
        Ok(())
    }
}
#[capnproto_rpc(table_ref)]
impl table_ref::Server for TableRefImpl {
    async fn appendonly(&self) {
        let client: r_a_table_ref::Client = self
            .db
            .server
            .capset
            .as_ref()
            .borrow_mut()
            .ra_table_ref_set
            .new_client(TableRefImpl {
                table_name: self.table_name.clone(),
                db: self.db.clone(),
            });
        results.get().set_res(client);
        Ok(())
    }
}
#[capnproto_rpc(table)]
impl table::Server for TableRefImpl {
    async fn adminless(&self) {
        let client: table_ref::Client = self
            .db
            .server
            .capset
            .as_ref()
            .borrow_mut()
            .table_ref_set
            .new_client(TableRefImpl {
                table_name: self.table_name.clone(),
                db: self.db.clone(),
            });
        results.get().set_res(client);
        Ok(())
    }
}
struct IndexImpl {
    name: Rc<String>,
    db: Rc<ServerDispatch<SqliteDatabase>>,
}

#[capnproto_rpc(index)]
impl index::Server for IndexImpl {}

#[capnproto_rpc(saveable)]
impl saveable::Server<index::Owned> for IndexImpl {
    async fn save(&self) {
        let id = self
            .db
            .get_string_index(crate::keystone::BUILTIN_SQLITE)
            .map_err(|e| capnp::Error::failed(e.to_string()))?;

        let mut msg = capnp::message::Builder::new_default();
        let mut builder = msg.init_root::<crate::sqlite_capnp::storage::Builder>();
        builder.set_id(INDEX_REF);
        builder.set_data(self.name.as_ref().as_str().into());

        let cap: sturdy_ref::Client<index::Owned> =
            self.db.sturdyref_set.borrow_mut().new_generic_client(
                crate::sturdyref::SturdyRefImpl::init(
                    id as u64,
                    builder.into_reader(),
                    self.db.clone(),
                )
                .map_err(|e| capnp::Error::failed(e.to_string()))?,
            );
        results.get().set_ref(cap);
        Ok(())
    }
}

#[capnproto_rpc(add_d_b)]
impl add_d_b::Server for SqliteDatabase {
    async fn create_table(&self, def: List) {
        let table = generate_table_name(
            self.this
                .upgrade()
                .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
        );
        let mut statement = String::new();
        statement.push_str("CREATE TABLE ");
        statement.push_str(table.table_name.as_str());
        statement.push_str(" (");
        statement.push_str("id INTEGER PRIMARY KEY, ");
        self.column_set.borrow_mut().insert("id".to_string());

        for field in def.iter() {
            let field_name = field.get_name()?.to_string()?;
            statement.push_str(field_name.as_str());
            self.column_set.borrow_mut().insert(field_name);
            match field.get_base_type()? {
                table_field::Type::Integer => statement.push_str(" INTEGER"),
                table_field::Type::Real => statement.push_str(" REAL"),
                table_field::Type::Text => statement.push_str(" TEXT"),
                table_field::Type::Blob => statement.push_str(" BLOB"),
                table_field::Type::Pointer => statement.push_str(" INTEGER"),
            }
            if !field.get_nullable() {
                statement.push_str(" NOT NULL");
            }
            statement.push_str(", ");
        }
        if statement.as_bytes()[statement.len() - 2] == b',' {
            statement.truncate(statement.len() - 2);
        }
        statement.push(')');
        self.connection
            .execute(statement.as_str(), ())
            .map_err(convert_rusqlite_error)?;
        let table_client: table::Client = capnp_rpc::new_client(table);
        results.get().set_res(table_client);
        Ok(())
    }
    async fn create_view(&self, names: List<Text>, def: Select) {
        let mut statement = String::new();
        statement.push_str("CREATE VIEW ");
        let view_name = create_view_name(
            self.this
                .upgrade()
                .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
        );
        statement.push_str(view_name.table_name.as_str());
        statement.push(' ');

        if !names.is_empty() {
            statement.push('(');
            for name in names.iter() {
                let name = name?.to_string()?;
                statement.push_str(name.as_str());
                statement.push_str(", ");
                self.column_set.borrow_mut().insert(name);
            }
            if statement.as_bytes()[statement.len() - 2] == b',' {
                statement.truncate(statement.len() - 2);
            }
            statement.push_str(") ");
        }
        statement.push_str("AS ");
        let statement_and_params =
            build_select_statement(self, def, StatementAndParams::new(80)).await?;
        statement.push_str(statement_and_params.statement.as_str());
        self.connection
            .execute(
                statement.as_str(),
                params_from_iter(statement_and_params.sql_params.iter()),
            )
            .map_err(convert_rusqlite_error)?;
        let client = self
            .capset
            .as_ref()
            .borrow_mut()
            .ro_table_ref_set
            .new_client(view_name);
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
        let index_name = create_index_name(
            self.this
                .upgrade()
                .ok_or(capnp::Error::failed("Database no longer exists".into()))?,
        );
        statement_and_params
            .statement
            .push_str(index_name.name.as_str());
        statement_and_params.statement.push_str(" ON ");
        let base = capnp::capability::get_resolved_cap(base).await;
        {
            let Some(server) = self
                .capset
                .as_ref()
                .borrow_mut()
                .table_ref_set
                .get_local_server_of_resolved(&base)
            else {
                return Err(capnp::Error::failed(
                    "Table ref invalid for this database or insufficient permissions".to_string(),
                ));
            };
            statement_and_params
                .statement
                .push_str(server.table_name.as_str());
            statement_and_params.statement.push_str(
                format!(" AS tableref{} ", statement_and_params.tableref_number).as_str(),
            );
            statement_and_params.tableref_number += 1;
        }
        statement_and_params.statement.push('(');
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
        if statement_and_params.statement.as_bytes()[statement_and_params.statement.len() - 2]
            == b','
        {
            statement_and_params
                .statement
                .truncate(statement_and_params.statement.len() - 2);
        }
        statement_and_params.statement.push(')');

        if params.get()?.has_sql_where() {
            statement_and_params.statement.push_str("WHERE ");
            match_where(
                self,
                params.get()?.get_sql_where()?,
                &mut statement_and_params,
            )
            .await?;
        }
        let mut stmt = self
            .connection
            .prepare(statement_and_params.statement.as_str())
            .map_err(convert_rusqlite_error)?;
        let _rows = stmt
            .query(params_from_iter(statement_and_params.sql_params.iter()))
            .map_err(convert_rusqlite_error)?;
        results
            .get()
            .set_res(self.index_set.borrow_mut().new_client(index_name));
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

fn generate_table_name(db: Rc<ServerDispatch<SqliteDatabase>>) -> TableRefImpl {
    let name = format!("table{}", rand::random::<u64>());
    TableRefImpl {
        table_name: Rc::new(name),
        db,
    }
}
fn create_index_name(db: Rc<ServerDispatch<SqliteDatabase>>) -> IndexImpl {
    let name = format!("index{}", rand::random::<u64>());
    IndexImpl {
        name: Rc::new(name),
        db,
    }
}
fn create_view_name(db: Rc<ServerDispatch<SqliteDatabase>>) -> TableRefImpl {
    let name = format!("view{}", rand::random::<u64>());
    TableRefImpl {
        table_name: Rc::new(name),
        db,
    }
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

async fn build_insert_statement<'a>(
    db: &SqliteDatabase,
    ins: insert::Reader<'a>,
    mut statement_and_params: StatementAndParams,
) -> capnp::Result<StatementAndParams> {
    let fallback_reader = ins.get_fallback()?;
    capnp_let!({target, cols, returning} = ins);
    let source = ins.get_source();

    statement_and_params.statement.push_str("INSERT OR ");
    match fallback_reader {
        insert::ConflictStrategy::Abort => statement_and_params.statement.push_str("ABORT INTO "),
        insert::ConflictStrategy::Fail => statement_and_params.statement.push_str("FAIL INTO "),
        insert::ConflictStrategy::Ignore => statement_and_params.statement.push_str("IGNORE INTO "),
        insert::ConflictStrategy::Rollback => {
            statement_and_params.statement.push_str("ROLLBACK INTO ")
        }
    };
    let target = capnp::capability::get_resolved_cap(target).await;

    {
        let Some(server) = db
            .capset
            .as_ref()
            .borrow()
            .ra_table_ref_set
            .get_local_server_of_resolved(&target)
        else {
            return Err(capnp::Error::failed(
                "Table ref invalid for this database or insufficient permissions".to_string(),
            ));
        };
        statement_and_params
            .statement
            .push_str(server.table_name.as_str());
        statement_and_params
            .statement
            .push_str(format!(" AS tableref{} ", statement_and_params.tableref_number).as_str());
        statement_and_params.tableref_number += 1;
    }

    statement_and_params.statement.push_str(" (");
    for col_name in cols.iter() {
        statement_and_params.statement.push_str(col_name?.to_str()?);
        statement_and_params.statement.push_str(", ");
    }
    if statement_and_params.statement.as_bytes()[statement_and_params.statement.len() - 2] == b',' {
        statement_and_params
            .statement
            .truncate(statement_and_params.statement.len() - 2);
    }
    statement_and_params.statement.push_str(") ");

    match source.which()? {
        insert::source::Which::Values(values) => {
            statement_and_params.statement.push_str("VALUES ");
            for value in values?.iter() {
                statement_and_params.statement.push('(');
                for dbany in value?.iter() {
                    match_dbany(db, dbany, &mut statement_and_params).await?;
                    statement_and_params.statement.push_str(", ");
                }
                statement_and_params
                    .statement
                    .truncate(statement_and_params.statement.len() - 2);
                statement_and_params.statement.push_str("), ");
            }
        }
        insert::source::Which::Select(select) => {
            statement_and_params =
                build_select_statement(db, select?, statement_and_params).await?;
        }
        insert::source::Which::Defaults(_) => {
            statement_and_params.statement.push_str("DEFAULT VALUES");
        }
    }
    if statement_and_params.statement.as_bytes()[statement_and_params.statement.len() - 2] == b',' {
        statement_and_params
            .statement
            .truncate(statement_and_params.statement.len() - 2);
    }

    if !returning.is_empty() {
        statement_and_params.statement.push_str(" RETURNING ");
        for expr in returning.iter() {
            match_expr(db, expr, &mut statement_and_params).await?;
            statement_and_params.statement.push_str(", ");
        }
        statement_and_params
            .statement
            .truncate(statement_and_params.statement.len() - 2);
    }
    Ok(statement_and_params)
}

async fn build_delete_statement<'a>(
    db: &SqliteDatabase,
    del: delete::Reader<'a>,
    mut statement_and_params: StatementAndParams,
) -> capnp::Result<StatementAndParams> {
    capnp_let!({from, returning} = del);
    statement_and_params.statement.push_str("DELETE FROM ");

    let tableref = capnp::capability::get_resolved_cap(from).await;
    {
        let Some(server) = db
            .capset
            .as_ref()
            .borrow_mut()
            .table_ref_set
            .get_local_server_of_resolved(&tableref)
        else {
            return Err(capnp::Error::failed(
                "Table ref invalid for this database or insufficient permissions".to_string(),
            ));
        };
        statement_and_params
            .statement
            .push_str(server.table_name.as_str());
        statement_and_params
            .statement
            .push_str(format!(" AS tableref{} ", statement_and_params.tableref_number).as_str());
        statement_and_params.tableref_number += 1;
    }

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
        statement_and_params
            .statement
            .truncate(statement_and_params.statement.len() - 2);
    }
    Ok(statement_and_params)
}
async fn build_update_statement<'a>(
    db: &SqliteDatabase,
    upd: update::Reader<'a>,
    mut statement_and_params: StatementAndParams,
) -> Result<StatementAndParams, capnp::Error> {
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
            {
                let Some(server) = db
                    .capset
                    .as_ref()
                    .borrow_mut()
                    .ro_table_ref_set
                    .get_local_server_of_resolved(&tableref)
                else {
                    return Err(capnp::Error::failed(
                        "Table ref invalid for this database or insufficient permissions"
                            .to_string(),
                    ));
                };
                statement_and_params
                    .statement
                    .push_str(server.table_name.as_str());
                statement_and_params.statement.push_str(
                    format!(" AS tableref{} ", statement_and_params.tableref_number).as_str(),
                );
                statement_and_params.tableref_number += 1;
            }
        }
        table_or_subquery::Which::Tablefunctioninvocation(func) => {
            let func = func?;
            let Some(server) = db
                .table_function_set
                .borrow()
                .get_local_server_of_resolved(&func.get_functionref()?)
            else {
                return Err(capnp::Error::failed(
                    "Table function ref invalid for this table or database".to_string(),
                ));
            };
            statement_and_params
                .statement
                .push_str(server.function.as_str());
            statement_and_params.statement.push_str(" (");
            for expr in func.get_exprs()? {
                match_expr(db, expr, &mut statement_and_params).await?;
                statement_and_params.statement.push_str(", ");
            }
            statement_and_params
                .statement
                .truncate(statement_and_params.statement.len() - 2);
            statement_and_params.statement.push_str(") ");
        }
        table_or_subquery::Which::Select(select) => {
            statement_and_params =
                build_select_statement(db, select?, statement_and_params).await?;
        }
        table_or_subquery::Which::Joinclause(join) => {
            statement_and_params = build_join_clause(db, join?, statement_and_params).await?;
        }
        table_or_subquery::Which::Null(()) => (),
    }

    if assignments.is_empty() {
        return Err(capnp::Error::failed(
            "Must provide at least one assignment".to_string(),
        ));
    } else {
        statement_and_params.statement.push_str(" SET ");
        for assignment in assignments.iter() {
            statement_and_params
                .statement
                .push_str(assignment.get_name()?.to_str()?);
            statement_and_params.statement.push_str(" = ");
            match_expr(db, assignment.get_expr()?, &mut statement_and_params).await?;
            statement_and_params.statement.push_str(", ");
        }
        statement_and_params
            .statement
            .truncate(statement_and_params.statement.len() - 2);
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
        statement_and_params
            .statement
            .truncate(statement_and_params.statement.len() - 2);
    }
    Ok(statement_and_params)
}
async fn build_select_statement<'a>(
    db: &SqliteDatabase,
    select: select::Reader<'a>,
    mut statement_and_params: StatementAndParams,
) -> Result<StatementAndParams, capnp::Error> {
    capnp_let!({names, selectcore : {from, results}, mergeoperations, orderby, limit} = select);
    statement_and_params.statement.push_str("SELECT ");
    let mut names_iter = names.iter();
    for expr in results.iter() {
        match expr.which()? {
            expr::Which::Literal(dbany) => match dbany?.which()? {
                d_b_any::Which::Null(_) => {
                    if !db.column_set.borrow_mut().contains("null") {
                        return Err(capnp::Error::failed(
                            "Invalid column specified in select clause results".to_string(),
                        ));
                    }
                    statement_and_params
                        .statement
                        .push_str(format!("{}, ", "null").as_str());
                }
                d_b_any::Which::Integer(int) => {
                    if !db.column_set.borrow_mut().contains(&int.to_string()) {
                        return Err(capnp::Error::failed(
                            "Invalid column specified in select clause results".to_string(),
                        ));
                    }
                    statement_and_params
                        .statement
                        .push_str(format!("{}, ", int).as_str());
                }
                d_b_any::Which::Real(real) => {
                    if !db.column_set.borrow_mut().contains(&real.to_string()) {
                        return Err(capnp::Error::failed(
                            "Invalid column specified in select clause results".to_string(),
                        ));
                    }
                    statement_and_params
                        .statement
                        .push_str(format!("{}, ", real).as_str());
                }
                d_b_any::Which::Text(text) => {
                    let string = text?.to_string()?;
                    if !db.column_set.borrow_mut().contains(&string) {
                        return Err(capnp::Error::failed(
                            "Invalid column specified in select clause results".to_string(),
                        ));
                    }
                    statement_and_params
                        .statement
                        .push_str(format!("{}, ", string).as_str());
                }
                d_b_any::Which::Blob(blob) => {
                    let string = std::str::from_utf8(blob?)?.to_owned();
                    if !db.column_set.borrow_mut().contains(&string) {
                        return Err(capnp::Error::failed(
                            "Invalid column specified in select clause results".to_string(),
                        ));
                    }
                    statement_and_params
                        .statement
                        .push_str(format!("{}, ", string).as_str());
                }
                d_b_any::Which::Pointer(pointer) => {
                    let response = pointer
                        .get_as_capability::<saveable::Client<capnp::any_pointer::Owned>>()?
                        .save_request()
                        .send()
                        .promise
                        .await?;
                    let client =
                        capnp::capability::get_resolved_cap(response.get()?.get_ref()?).await;
                    let sturdyref = db
                        .sturdyref_set
                        .borrow_mut()
                        .get_local_server_of_resolved(&client)
                        .ok_or(capnp::Error::failed(
                            "Sturdyref does not belong to this instance!".into(),
                        ))?;

                    let id = sturdyref.server.get_id();
                    if !db.column_set.borrow_mut().contains(&id.to_string()) {
                        return Err(capnp::Error::failed(
                            "Invalid column specified in select clause results".to_string(),
                        ));
                    }

                    statement_and_params
                        .statement
                        .push_str(format!("{}, ", id).as_str());
                }
            },
            expr::Which::Bindparam(_) => {
                return Err(capnp::Error::failed(
                    "Select result can't be a bindparam".into(),
                ));
            }
            expr::Which::Tablereference(table_column) => {
                let table_column = table_column?;
                statement_and_params.statement.push_str(
                    format!(
                        "tableref{}.{}, ",
                        table_column.get_reference(),
                        table_column.get_col_name()?.to_str()?
                    )
                    .as_str(),
                );
            }
            expr::Which::Functioninvocation(func) => {
                build_function_invocation(db, func?, &mut statement_and_params).await?;
                statement_and_params.statement.push_str(", ");
            }
        }
        if let Some(name) = names_iter.next() {
            if statement_and_params.statement.as_bytes()[statement_and_params.statement.len() - 2]
                == b','
            {
                statement_and_params
                    .statement
                    .truncate(statement_and_params.statement.len() - 2);
            }
            statement_and_params.statement.push_str(" AS ");
            statement_and_params.statement.push_str(name?.to_str()?);
            statement_and_params.statement.push_str(", ");
        }
    }
    if statement_and_params.statement.as_bytes()[statement_and_params.statement.len() - 2] == b',' {
        statement_and_params
            .statement
            .truncate(statement_and_params.statement.len() - 2);
    }

    if from.has_tableorsubquery() || from.has_joinoperations() {
        statement_and_params.statement.push_str(" FROM ");
        statement_and_params = Box::pin(build_join_clause(db, from, statement_and_params)).await?;
    }
    if selectcore.has_sql_where() {
        statement_and_params.statement.push_str(" WHERE ");
        match_where(db, selectcore.get_sql_where()?, &mut statement_and_params).await?;
        statement_and_params.statement.push(' ');
    }

    for merge_operation in mergeoperations.iter() {
        match merge_operation.get_operator()? {
            select::merge_operation::MergeOperator::Union => {
                statement_and_params.statement.push_str(" UNION ")
            }
            select::merge_operation::MergeOperator::Unionall => {
                statement_and_params.statement.push_str(" UNION ALL ")
            }
            select::merge_operation::MergeOperator::Intersect => {
                statement_and_params.statement.push_str(" INTERSECT ")
            }
            select::merge_operation::MergeOperator::Except => {
                statement_and_params.statement.push_str(" EXCEPT ")
            }
        }
        statement_and_params =
            Box::pin(build_select_statement(db, select, statement_and_params)).await?;
    }
    if !orderby.is_empty() {
        statement_and_params.statement.push_str(" ORDER BY ");
        for term in orderby.iter() {
            match_expr(db, term.get_expr()?, &mut statement_and_params).await?;
            match term.get_direction()? {
                select::ordering_term::AscDesc::Asc => {
                    statement_and_params.statement.push_str(" ASC")
                }
                select::ordering_term::AscDesc::Desc => {
                    statement_and_params.statement.push_str(" DSC")
                }
            }
            statement_and_params.statement.push_str(", ");
        }
        statement_and_params
            .statement
            .truncate(statement_and_params.statement.len() - 2);
    }
    if limit.has_limit() {
        statement_and_params.statement.push_str("LIMIT ");
        match_expr(db, limit.get_limit()?, &mut statement_and_params).await?;
        statement_and_params.statement.push(' ');
    }
    if limit.has_offset() {
        statement_and_params.statement.push_str("OFFSET ");
        match_expr(db, limit.get_offset()?, &mut statement_and_params).await?;
        statement_and_params.statement.push(' ');
    }

    Ok(statement_and_params)
}
async fn build_function_invocation<'a>(
    db: &SqliteDatabase,
    function_reader: function_invocation::Reader<'a>,
    statement_and_params: &mut StatementAndParams,
) -> Result<(), capnp::Error> {
    let Some(server) = db
        .sql_function_set
        .borrow()
        .get_local_server_of_resolved(&function_reader.reborrow().get_function()?)
    else {
        return Err(capnp::Error::failed("Sql function cap invalid".to_string()));
    };
    statement_and_params
        .statement
        .push_str(server.function.as_str());
    if function_reader.has_params() {
        statement_and_params.statement.push_str(" (");
        for param in function_reader.get_params()?.iter() {
            Box::pin(match_expr(db, param, statement_and_params)).await?;
            statement_and_params.statement.push_str(", ");
        }
        statement_and_params
            .statement
            .truncate(statement_and_params.statement.len() - 2);
        statement_and_params.statement.push(')');
    }
    Ok(())
}
async fn build_join_clause<'a>(
    db: &SqliteDatabase,
    join_clause: join_clause::Reader<'a>,
    mut statement_and_params: StatementAndParams,
) -> Result<StatementAndParams, capnp::Error> {
    match join_clause.get_tableorsubquery()?.which()? {
        table_or_subquery::Which::Tableref(tableref) => {
            let tableref = capnp::capability::get_resolved_cap(tableref?).await;
            {
                let Some(server) = db
                    .capset
                    .as_ref()
                    .borrow_mut()
                    .ro_table_ref_set
                    .get_local_server_of_resolved(&tableref)
                else {
                    return Err(capnp::Error::failed(
                        "Table ref invalid for this database or insufficient permissions"
                            .to_string(),
                    ));
                };
                statement_and_params
                    .statement
                    .push_str(server.table_name.as_str());
                statement_and_params.statement.push_str(
                    format!(" AS tableref{} ", statement_and_params.tableref_number).as_str(),
                );
                statement_and_params.tableref_number += 1;
            }
        }
        table_or_subquery::Which::Tablefunctioninvocation(func) => {
            let func = func?;
            let Some(server) = db
                .table_function_set
                .borrow()
                .get_local_server_of_resolved(&func.get_functionref()?)
            else {
                return Err(capnp::Error::failed(
                    "Table function ref invalid for this table or database".to_string(),
                ));
            };
            statement_and_params
                .statement
                .push_str(server.function.as_str());
            statement_and_params.statement.push_str(" (");
            for expr in func.get_exprs()? {
                match_expr(db, expr, &mut statement_and_params).await?;
                statement_and_params.statement.push_str(", ");
            }
            statement_and_params
                .statement
                .truncate(statement_and_params.statement.len() - 2);
            statement_and_params.statement.push_str(") ");
        }
        table_or_subquery::Which::Select(select) => {
            statement_and_params =
                build_select_statement(db, select?, statement_and_params).await?;
        }
        table_or_subquery::Which::Joinclause(join) => {
            statement_and_params =
                Box::pin(build_join_clause(db, join?, statement_and_params)).await?;
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
                    join_clause::join_operation::join_operator::JoinParameter::Left => {
                        statement_and_params.statement.push_str("LEFT OUTER ")
                    }
                    join_clause::join_operation::join_operator::JoinParameter::Right => {
                        statement_and_params.statement.push_str("RIGHT OUTER ")
                    }
                    join_clause::join_operation::join_operator::JoinParameter::Full => {
                        statement_and_params.statement.push_str("FULL OUTER ")
                    }
                    join_clause::join_operation::join_operator::JoinParameter::None => (),
                }
                statement_and_params.statement.push_str("JOIN ");
            }
            join_clause::join_operation::join_operator::Which::PlainJoin(p) => {
                match p? {
                    join_clause::join_operation::join_operator::JoinParameter::Left => {
                        statement_and_params.statement.push_str("LEFT ")
                    }
                    join_clause::join_operation::join_operator::JoinParameter::Right => {
                        statement_and_params.statement.push_str("RIGHT ")
                    }
                    join_clause::join_operation::join_operator::JoinParameter::Full => {
                        statement_and_params.statement.push_str("FULL ")
                    }
                    join_clause::join_operation::join_operator::JoinParameter::None => (),
                }
                statement_and_params.statement.push_str("JOIN ");
            }
        }
        match op.get_tableorsubquery()?.which()? {
            table_or_subquery::Which::Tableref(tableref) => {
                let tableref = capnp::capability::get_resolved_cap(tableref?).await;
                {
                    let Some(server) = db
                        .capset
                        .as_ref()
                        .borrow_mut()
                        .ro_table_ref_set
                        .get_local_server_of_resolved(&tableref)
                    else {
                        return Err(capnp::Error::failed(
                            "Table ref invalid for this database or insufficient permissions"
                                .to_string(),
                        ));
                    };
                    statement_and_params
                        .statement
                        .push_str(server.table_name.as_str());
                    statement_and_params.statement.push_str(
                        format!(" AS tableref{} ", statement_and_params.tableref_number).as_str(),
                    );
                    statement_and_params.tableref_number += 1;
                }
            }
            table_or_subquery::Which::Tablefunctioninvocation(func) => {
                let func = func?;
                let Some(server) = db
                    .table_function_set
                    .borrow()
                    .get_local_server_of_resolved(&func.get_functionref()?)
                else {
                    return Err(capnp::Error::failed(
                        "Table function ref invalid for this table or database".to_string(),
                    ));
                };
                statement_and_params
                    .statement
                    .push_str(server.function.as_str());
                statement_and_params.statement.push_str(" (");
                for expr in func.get_exprs()? {
                    match_expr(db, expr, &mut statement_and_params).await?;
                    statement_and_params.statement.push_str(", ");
                }
                statement_and_params
                    .statement
                    .truncate(statement_and_params.statement.len() - 2);
                statement_and_params.statement.push_str(") ");
            }
            table_or_subquery::Which::Select(select) => {
                statement_and_params =
                    build_select_statement(db, select?, statement_and_params).await?;
            }
            table_or_subquery::Which::Joinclause(join) => {
                statement_and_params =
                    Box::pin(build_join_clause(db, join?, statement_and_params)).await?;
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
                statement_and_params.statement.push('(');
                for col_name in cols?.iter() {
                    statement_and_params
                        .statement
                        .push_str(format!("{}, ", col_name?.to_str()?).as_str());
                }
                statement_and_params
                    .statement
                    .truncate(statement_and_params.statement.len() - 2);
                statement_and_params.statement.push(')');
            }
            join_clause::join_operation::join_constraint::Which::Empty(_) => (),
        }
    }
    Ok(statement_and_params)
}
fn build_results_stream_buffer(mut rows: rusqlite::Rows<'_>) -> capnp::Result<Vec<Vec<SqlDBAny>>> {
    let mut row_vec = Vec::new();
    while let Ok(Some(row)) = rows.next() {
        let mut value_vec = Vec::new();
        let mut i = 0;
        while let Ok(value) = row.get_ref(i) {
            match value {
                rusqlite::types::ValueRef::Null => value_vec.push(SqlDBAny::None),
                rusqlite::types::ValueRef::Integer(int) => value_vec.push(SqlDBAny::Int(int)),
                rusqlite::types::ValueRef::Real(r) => value_vec.push(SqlDBAny::Real(r)),
                rusqlite::types::ValueRef::Text(t) => {
                    value_vec.push(SqlDBAny::Str(std::str::from_utf8(t)?.to_string()))
                }
                rusqlite::types::ValueRef::Blob(b) => value_vec.push(SqlDBAny::Blob(b.to_vec())),
            }
            i += 1;
        }
        row_vec.push(value_vec)
    }
    Ok(row_vec)
}
async fn match_expr<'a>(
    db: &SqliteDatabase,
    expr: expr::Reader<'a>,
    statement_and_params: &mut StatementAndParams,
) -> capnp::Result<()> {
    match expr.which()? {
        expr::Which::Literal(dbany) => {
            match_dbany(db, dbany?, statement_and_params).await?;
        }
        expr::Which::Bindparam(_) => {
            statement_and_params
                .sql_params
                .push(rusqlite::types::Value::Null);
            statement_and_params
                .bindparam_indexes
                .push(statement_and_params.sql_params.len() - 1);
            statement_and_params
                .statement
                .push_str(format!("?{}", statement_and_params.sql_params.len()).as_str());
        }
        expr::Which::Tablereference(table_column) => {
            let table_column = table_column?;
            statement_and_params.statement.push_str(
                format!(
                    "tableref{}.{}",
                    table_column.get_reference(),
                    table_column.get_col_name()?.to_str()?
                )
                .as_str(),
            );
        }
        expr::Which::Functioninvocation(func) => {
            build_function_invocation(db, func?, statement_and_params).await?;
        }
    }
    Ok(())
}
async fn match_dbany<'a>(
    db: &SqliteDatabase,
    dbany: d_b_any::Reader<'a>,
    statement_and_params: &mut StatementAndParams,
) -> capnp::Result<()> {
    fn inner(statement_and_params: &mut StatementAndParams, value: rusqlite::types::Value) {
        statement_and_params.sql_params.push(value);
        statement_and_params
            .statement
            .push_str(format!("?{}", statement_and_params.sql_params.len()).as_str());
    }

    let value = match dbany.which()? {
        d_b_any::Which::Null(_) => rusqlite::types::Value::Null,
        d_b_any::Which::Integer(int) => rusqlite::types::Value::Integer(int),
        d_b_any::Which::Real(real) => rusqlite::types::Value::Real(real),
        d_b_any::Which::Text(text) => rusqlite::types::Value::Text(text?.to_string()?),
        d_b_any::Which::Blob(blob) => rusqlite::types::Value::Blob(blob?.to_vec()),
        d_b_any::Which::Pointer(pointer) => {
            let response = pointer
                .get_as_capability::<saveable::Client<capnp::any_pointer::Owned>>()?
                .save_request()
                .send()
                .promise
                .await?;
            let client = capnp::capability::get_resolved_cap(response.get()?.get_ref()?).await;
            let sturdyref = db
                .sturdyref_set
                .borrow_mut()
                .get_local_server_of_resolved(&client)
                .ok_or(capnp::Error::failed(
                    "Sturdyref does not belong to this instance!".into(),
                ))?;

            rusqlite::types::Value::Integer(sturdyref.server.get_id())
        }
    };

    inner(statement_and_params, value);
    Ok(())
}
async fn match_where<'a>(
    db: &SqliteDatabase,
    w_expr: where_expr::Reader<'a>,
    statement_and_params: &mut StatementAndParams,
) -> capnp::Result<()> {
    let cols = w_expr.get_cols()?;
    let cols_len = cols.len();
    if cols_len == 0 {
        return Err(capnp::Error::failed(
            "Where clause does not have any column names specified".to_string(),
        ));
    }
    if cols_len > 1 {
        statement_and_params.statement.push('(');
    }
    for column in cols.iter() {
        let column = column?.to_string()?;
        if !db.column_set.borrow_mut().contains(&column) {
            return Err(capnp::Error::failed(
                "Invalid column specified in where clause".to_string(),
            ));
        }
        statement_and_params
            .statement
            .push_str(format!("{}, ", column).as_str());
    }
    statement_and_params
        .statement
        .truncate(statement_and_params.statement.len() - 2);
    if cols_len > 1 {
        statement_and_params.statement.push(')');
    }
    let operator_and_expr = w_expr.get_operator_and_expr()?;
    if operator_and_expr.is_empty() {
        return Err(capnp::Error::failed(
            "Where clause is missing operator and condition".to_string(),
        ));
    }
    for w_expr in operator_and_expr.iter() {
        match w_expr.get_operator()? {
            where_expr::Operator::Is => statement_and_params.statement.push_str(" IS "),
            where_expr::Operator::IsNot => statement_and_params.statement.push_str(" IS NOT "),
            where_expr::Operator::And => statement_and_params.statement.push_str(" AND "),
            where_expr::Operator::Or => statement_and_params.statement.push_str(" OR "),
        }
        if !w_expr.has_expr() {
            return Err(capnp::Error::failed(
                "Where clause is missing condition".to_string(),
            ));
        }
        match_expr(db, w_expr.get_expr()?, statement_and_params).await?;
    }
    Ok(())
}
async fn fill_in_bindparams<'a>(
    db: &SqliteDatabase,
    bindparam_indexes: &Vec<usize>,
    params: &mut [rusqlite::types::Value],
    bindings_reader: capnp::struct_list::Reader<'a, d_b_any::Owned>,
) -> capnp::Result<()> {
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
                    let response = pointer
                        .get_as_capability::<saveable::Client<capnp::any_pointer::Owned>>()?
                        .save_request()
                        .send()
                        .promise
                        .await?;
                    let client =
                        capnp::capability::get_resolved_cap(response.get()?.get_ref()?).await;
                    let sturdyref = db
                        .sturdyref_set
                        .borrow_mut()
                        .get_local_server_of_resolved(&client)
                        .ok_or(capnp::Error::failed(
                            "Sturdyref does not belong to this instance!".into(),
                        ))?;

                    rusqlite::types::Value::Integer(sturdyref.server.get_id())
                }
            }
        } else {
            return Err(capnp::Error::failed(
                "Not enough params provided for binding slots specified in prepare statement"
                    .to_string(),
            ));
        }
    }
    if bindings_iter.next().is_some() {
        return Err(capnp::Error::failed(
            "Too many params provided for binding slots specified in prepare statement".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use capnp::capability::FromClientHook;
    use capnp::private::capability::ClientHook;
    use tempfile::NamedTempFile;

    use super::*;
    #[tokio::test]
    async fn test_sqlite() -> eyre::Result<()> {
        let db_path = NamedTempFile::new().unwrap().into_temp_path();
        let hook = capnp_rpc::local::Client::from_rc(SqliteDatabase::new(
            db_path.to_path_buf(),
            OpenFlags::default(),
            Default::default(),
            Default::default(),
        )?)
        .add_ref();
        let client: add_d_b::Client = capnp::capability::FromClientHook::new(hook);

        let create_table_request = client.build_create_table_request(vec![
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

        let ro_tableref_cap = ra_table_ref_cap
            .readonly_request()
            .send()
            .promise
            .await?
            .get()?
            .get_res()?;

        let insert_request = client
            .clone()
            .cast_to::<database::Client>()
            .build_insert_request(Some(insert::Insert {
                _fallback: insert::ConflictStrategy::Fail,
                _target: ra_table_ref_cap.clone(),
                _source: source::Source::_Values(vec![vec![
                    DBAny::_Text("Steven".to_string()),
                    DBAny::_Null(()),
                ]]),
                _cols: vec!["name".to_string(), "data".to_string()],
                _returning: Vec::new(),
            }));
        insert_request.send().promise.await?;

        let insert_request = client
            .clone()
            .cast_to::<database::Client>()
            .build_insert_request(Some(insert::Insert {
                _fallback: insert::ConflictStrategy::Abort,
                _target: ra_table_ref_cap.clone(),
                _source: source::Source::_Values(vec![vec![
                    DBAny::_Text("ToUpdate".to_string()),
                    DBAny::_Blob(vec![4, 5, 6]),
                ]]),
                _cols: vec!["name".to_string(), "data".to_string()],
                _returning: Vec::new(),
            }));
        insert_request.send().promise.await?;

        let update_request = client
            .clone()
            .cast_to::<database::Client>()
            .build_update_request(Some(update::Update {
                _fallback: update::ConflictStrategy::Fail,
                _assignments: vec![
                    update::assignment::Assignment {
                        _name: "name".to_string(),
                        _expr: Some(expr::Expr::_Literal(DBAny::_Text("Updated".to_string()))),
                    },
                    update::assignment::Assignment {
                        _name: "data".to_string(),
                        _expr: Some(expr::Expr::_Literal(DBAny::_Null(()))),
                    },
                ],
                _from: Some(join_clause::JoinClause {
                    _tableorsubquery: Some(table_or_subquery::TableOrSubquery::_Tableref(
                        ro_tableref_cap.clone(),
                    )),
                    _joinoperations: Vec::new(),
                }),
                _sql_where: Some(where_expr::WhereExpr {
                    _cols: vec!["name".to_string()],
                    _operator_and_expr: vec![where_expr::op_and_expr::OpAndExpr {
                        _operator: where_expr::Operator::Is,
                        _expr: Some(expr::Expr::_Literal(DBAny::_Text("ToUpdate".to_string()))),
                    }],
                }),
                _returning: Vec::new(),
            }));
        update_request.send().promise.await?;

        let prepare_insert_request = client
            .clone()
            .cast_to::<database::Client>()
            .build_prepare_insert_request(Some(insert::Insert {
                _fallback: insert::ConflictStrategy::Ignore,
                _target: ra_table_ref_cap.clone(),
                _cols: vec!["name".to_string(), "data".to_string()],
                _source: insert::source::Source::_Values(vec![vec![
                    DBAny::_Text("Mike".to_string()),
                    DBAny::_Blob(vec![1, 2, 3]),
                ]]),
                _returning: vec![expr::Expr::_Bindparam(())],
            }));
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

        let select_request = client
            .clone()
            .cast_to::<r_o_database::Client>()
            .build_select_request(Some(select::Select {
                _selectcore: Some(Box::new(select_core::SelectCore {
                    _from: Some(join_clause::JoinClause {
                        _tableorsubquery: Some(table_or_subquery::TableOrSubquery::_Tableref(
                            ro_tableref_cap,
                        )),
                        _joinoperations: Vec::new(),
                    }),
                    _results: vec![
                        expr::Expr::_Literal(DBAny::_Text("id".to_string())),
                        expr::Expr::_Literal(DBAny::_Text("name".to_string())),
                        expr::Expr::_Literal(DBAny::_Text("data".to_string())),
                    ],
                    _sql_where: None,
                })),
                _mergeoperations: Vec::new(),
                _orderby: Vec::new(),
                _limit: None,
                _names: Vec::new(),
            }));

        let res_stream = select_request.send().promise.await?.get()?.get_res()?;
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
        delete_from_table_request.send().promise.await?;

        Ok(())
    }
}

fn convert_rusqlite_error(err: rusqlite::Error) -> capnp::Error {
    // When we are testing things, output the actual sqlite error
    #[cfg(test)]
    return capnp::Error::failed(err.to_string());

    match err {
        rusqlite::Error::SqliteFailure(_, _) => {
            capnp::Error::failed("Error from underlying sqlite call".to_string())
        }
        rusqlite::Error::SqliteSingleThreadedMode => capnp::Error::failed(
            "Attempting to open multiple connection when sqlite is in single threaded mode"
                .to_string(),
        ),
        rusqlite::Error::FromSqlConversionFailure(_, _, _) => {
            capnp::Error::failed("Error converting sql type to rust type".to_string())
        }
        rusqlite::Error::IntegralValueOutOfRange(_, _) => {
            capnp::Error::failed("Integral value out of range".to_string())
        }
        rusqlite::Error::Utf8Error(e) => {
            capnp::Error::from_kind(capnp::ErrorKind::TextContainsNonUtf8Data(e))
        }
        rusqlite::Error::NulError(_) => capnp::Error::failed(
            "Error converting string to c-compatible strting, because it contains an embeded null"
                .to_string(),
        ),
        rusqlite::Error::InvalidParameterName(_) => {
            capnp::Error::failed("Invalid parameter name".to_string())
        }
        rusqlite::Error::InvalidPath(_) => capnp::Error::failed("Invalid path".to_string()),
        rusqlite::Error::ExecuteReturnedResults => {
            capnp::Error::failed("Execute call returned rows".to_string())
        }
        rusqlite::Error::QueryReturnedNoRows => capnp::Error::failed(
            "Query that was expected to return rows returned no rows".to_string(),
        ),
        rusqlite::Error::InvalidColumnIndex(_) => {
            capnp::Error::failed("Invalid column index".to_string())
        }
        rusqlite::Error::InvalidColumnName(_) => {
            capnp::Error::failed("Invalid column name".to_string())
        }
        rusqlite::Error::InvalidColumnType(_, _, _) => {
            capnp::Error::failed("Invalid column type".to_string())
        }
        rusqlite::Error::StatementChangedRows(_) => {
            capnp::Error::failed("Query changed more/less rows than expected".to_string())
        }
        rusqlite::Error::ToSqlConversionFailure(_) => {
            capnp::Error::failed("Failed to convert type to sql type".to_string())
        }
        rusqlite::Error::InvalidQuery => capnp::Error::failed("Invalid query".to_string()),
        rusqlite::Error::MultipleStatement => {
            capnp::Error::failed("Sql contains multiple statements".to_string())
        }
        rusqlite::Error::InvalidParameterCount(_, _) => {
            capnp::Error::failed("Invalid parameter count".to_string())
        }
        rusqlite::Error::SqlInputError {
            error: _,
            msg: _,
            sql: _,
            offset: _,
        } => capnp::Error::failed("Invalid SQL syntax".to_string()),
        _ => capnp::Error::failed("Sqlite error".to_string()),
    }
}
