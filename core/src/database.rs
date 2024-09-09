use crate::buffer_allocator::BufferAllocator;
use crate::sqlite::SqliteDatabase;
use crate::storage_capnp::restore::restore_results;
use capnp::any_pointer::Owned as any_pointer;
use capnp::capability::FromClientHook;
use capnp::message::{ReaderOptions, TypedReader};
use capnp::traits::SetPointerBuilder;
use eyre::Result;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::rc::Rc;

/// Internal trait that extends our internal database object with keystone-specific actions
#[allow(dead_code)]
pub trait DatabaseExt {
    fn init(&self) -> Result<()>;

    fn get_state(
        &self,
        string_index: i64,
        builder: capnp::dynamic_value::Builder<'_>,
    ) -> Result<()>;

    fn set_state<R: SetPointerBuilder + Clone>(&self, string_index: i64, data: R) -> Result<()>;

    fn init_state<R: SetPointerBuilder + Clone>(&self, string_index: i64, data: R) -> Result<()>;

    fn get_sturdyref(
        &self,
        sturdy_id: i64,
    ) -> Result<capnp::capability::RemotePromise<restore_results::Owned<any_pointer, any_pointer>>>;
    fn add_sturdyref<R: SetPointerBuilder + Clone>(&self, module_id: u64, data: R) -> Result<i64>;
    fn drop_sturdyref(&self, id: i64) -> Result<()>;
    fn add_object<R: SetPointerBuilder + Clone>(&self, data: R) -> Result<i64>;
    fn get_object(&self, id: i64, builder: capnp::dynamic_value::Builder<'_>) -> Result<()>;
    fn drop_object(&self, id: i64) -> Result<()>;
    fn get_string_index(&self, s: &str) -> Result<i64>;
}

// Rust's type system does not like enums in generic constants
const ROOT_STATES: usize = 0;
const ROOT_STURDYREFS: usize = 1;
const ROOT_OBJECTS: usize = 2;
const ROOT_TABLES: &[&str] = &["states", "sturdyrefs", "objects"];

fn expect_change(call: Result<usize, rusqlite::Error>, count: usize) -> Result<()> {
    let n = call?;
    if n != count {
        Err(rusqlite::Error::StatementChangedRows(n).into())
    } else {
        Ok(())
    }
}

fn get_generic<const TABLE: usize>(
    conn: &Connection,
    id: i64,
    builder: capnp::dynamic_value::Builder<'_>,
) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        format!("SELECT data FROM {} WHERE id = ?1", ROOT_TABLES[TABLE]).as_str(),
    )?;
    stmt.query_row(
        params![id],
        |row: &rusqlite::Row| -> Result<(), rusqlite::Error> {
            let v = row.get_ref(0)?;
            let slice = [v.as_blob()?];

            let message_reader = TypedReader::<_, any_pointer>::new(capnp::message::Reader::new(
                capnp::message::SegmentArray::new(&slice),
                ReaderOptions {
                    traversal_limit_in_words: None,
                    nesting_limit: 128,
                },
            ));
            let reader: capnp::any_pointer::Reader = message_reader
                .get()
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

            match builder {
                capnp::dynamic_value::Builder::Struct(mut s) => s
                    .copy_from(reader)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e))),
                capnp::dynamic_value::Builder::AnyPointer(mut b) => b
                    .set_as(reader)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e))),
                _ => Err(rusqlite::Error::InvalidQuery),
            }
        },
    )?;
    Ok(())
}

fn get_single_segment<R: SetPointerBuilder + Clone>(
    alloc: &mut BufferAllocator,
    data: R,
) -> capnp::Result<capnp::message::Builder<&mut BufferAllocator>> {
    let mut message = capnp::message::Builder::new(alloc);
    message.set_root(data.clone())?;

    if let capnp::OutputSegments::MultiSegment(v) = message.get_segments_for_output() {
        let n = v.into_iter().fold(0, |i, v| i + v.len()) * 2;
        let a = message.into_allocator();
        a.reserve(n);
        message = capnp::message::Builder::new(a);
        message.set_root(data)?;
    }

    Ok(message)
}

fn get_segment_slice<'a>(
    message: &'a capnp::message::Builder<&mut BufferAllocator>,
) -> capnp::Result<&'a [u8]> {
    let slice = match message.get_segments_for_output() {
        capnp::OutputSegments::SingleSegment(s) => s[0],
        capnp::OutputSegments::MultiSegment(v) => {
            return Err(
                capnp::Error::from_kind(capnp::ErrorKind::InvalidNumberOfSegments(v.len())).into(),
            );
        }
    };

    Ok(slice)
}

fn set_generic<const TABLE: usize, const UPDATE: bool, R: SetPointerBuilder + Clone>(
    conn: &Connection,
    mut alloc: std::cell::RefMut<BufferAllocator>,
    string_index: i64,
    data: R,
) -> Result<()> {
    let message = get_single_segment(&mut *alloc, data)?;
    let slice = get_segment_slice(&message)?;

    if UPDATE {
        let call = conn.prepare_cached(
            format!("INSERT INTO {} (id, data) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET data=?2", ROOT_TABLES[TABLE]).as_str())?.execute(
            params![string_index, slice]
        );

        expect_change(call, 1)
    } else {
        let call = conn
            .prepare_cached(
                format!(
                    "INSERT OR IGNORE INTO {} (id, data) VALUES (?1, ?2)",
                    ROOT_TABLES[TABLE]
                )
                .as_str(),
            )?
            .execute(params![string_index, slice])?;

        if call > 1 {
            Err(rusqlite::Error::StatementChangedRows(call).into())
        } else {
            Ok(())
        }
    }
}

fn add_generic<const TABLE: usize, R: SetPointerBuilder + Clone>(
    conn: &Connection,
    mut alloc: std::cell::RefMut<BufferAllocator>,
    data: R,
) -> Result<i64> {
    let message = get_single_segment(&mut *alloc, data)?;
    let slice = get_segment_slice(&message)?;

    let result = conn
        .prepare_cached(format!("INSERT INTO {} (data) VALUES (?1)", ROOT_TABLES[TABLE]).as_str())?
        .execute(params![slice]);
    expect_change(result, 1)?;
    Ok(conn.last_insert_rowid())
}

fn drop_generic<const TABLE: usize>(conn: &Connection, id: i64) -> Result<()> {
    expect_change(
        conn.prepare_cached(format!("DELETE FROM {} WHERE id = ?1", ROOT_TABLES[TABLE]).as_str())?
            .execute(params![id]),
        1,
    )
}

impl DatabaseExt for SqliteDatabase {
    fn init(&self) -> Result<()> {
        // These three tables look the same but we interact with them slightly differently
        for t in [ROOT_STATES, ROOT_OBJECTS] {
            self.connection.execute(
                format!(
                    "CREATE TABLE {} (
                        id    INTEGER PRIMARY KEY,
                        data  BLOB NOT NULL
                    )",
                    ROOT_TABLES[t]
                )
                .as_str(),
                (),
            )?;
        }

        self.connection.execute(
            format!(
                "CREATE TABLE {} (
                    id     INTEGER PRIMARY KEY,
                    module INTEGER NOT NULL    
                    data   BLOB NOT NULL
                )",
                ROOT_TABLES[ROOT_STURDYREFS]
            )
            .as_str(),
            (),
        )?;

        self.connection.execute(
            "CREATE TABLE stringmap (
            id    INTEGER PRIMARY KEY,
            string  TEXT UNIQUE NOT NULL
        )",
            (),
        )?;

        Ok(())
    }

    fn get_state(
        &self,
        string_index: i64,
        builder: capnp::dynamic_value::Builder<'_>,
    ) -> Result<()> {
        get_generic::<ROOT_STATES>(&self.connection, string_index, builder)
    }

    fn set_state<R: SetPointerBuilder + Clone>(&self, string_index: i64, data: R) -> Result<()> {
        set_generic::<ROOT_STATES, true, R>(
            &self.connection,
            self.alloc.borrow_mut(),
            string_index,
            data,
        )
    }

    fn init_state<R: SetPointerBuilder + Clone>(&self, string_index: i64, data: R) -> Result<()> {
        set_generic::<ROOT_STATES, false, R>(
            &self.connection,
            self.alloc.borrow_mut(),
            string_index,
            data,
        )
    }

    fn get_sturdyref(
        &self,
        id: i64,
    ) -> Result<capnp::capability::RemotePromise<restore_results::Owned<any_pointer, any_pointer>>>
    {
        let mut stmt = self.connection.prepare_cached(
            format!(
                "SELECT data, module FROM {} WHERE id = ?1",
                ROOT_TABLES[ROOT_STURDYREFS]
            )
            .as_str(),
        )?;
        let promise = stmt.query_row(
            params![id],
            |row: &rusqlite::Row| -> Result<
                capnp::capability::RemotePromise<restore_results::Owned<any_pointer, any_pointer>>,
                rusqlite::Error,
            > {
                let id_ref = row.get_ref(1)?;
                let module_id = id_ref.as_i64()? as u64;
                let borrow = self.clients.borrow_mut();
                let hook =
                    borrow
                        .get(&module_id)
                        .ok_or(rusqlite::Error::IntegralValueOutOfRange(
                            0,
                            module_id as i64,
                        ))?;
                let client =
                    crate::storage_capnp::restore::Client::<any_pointer>::new(hook.add_ref());

                let v = row.get_ref(0)?;
                let slice = [v.as_blob()?];

                let message_reader =
                    TypedReader::<_, any_pointer>::new(capnp::message::Reader::new(
                        capnp::message::SegmentArray::new(&slice),
                        ReaderOptions {
                            traversal_limit_in_words: None,
                            nesting_limit: 128,
                        },
                    ));
                let reader: capnp::any_pointer::Reader = message_reader
                    .get()
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

                let mut req = client.restore_request();
                req.get()
                    .set_data(reader)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                Ok(req.send())
            },
        )?;
        Ok(promise)
    }

    fn add_sturdyref<R: SetPointerBuilder + Clone>(&self, module_id: u64, data: R) -> Result<i64> {
        let mut alloc = self.alloc.borrow_mut();
        let message = get_single_segment(&mut *alloc, data)?;
        let slice = get_segment_slice(&message)?;

        let result = self
            .connection
            .prepare_cached(
                format!(
                    "INSERT INTO {} (module, data) VALUES (?1, ?2)",
                    ROOT_TABLES[ROOT_STURDYREFS]
                )
                .as_str(),
            )?
            .execute(params![module_id, slice]);
        expect_change(result, 1)?;
        Ok(self.connection.last_insert_rowid())
    }

    fn drop_sturdyref(&self, id: i64) -> Result<()> {
        drop_generic::<ROOT_STURDYREFS>(&self.connection, id)
    }

    fn add_object<R: SetPointerBuilder + Clone>(&self, data: R) -> Result<i64> {
        let mut alloc = self.alloc.borrow_mut();
        let message = get_single_segment(&mut *alloc, data)?;
        let slice = get_segment_slice(&message)?;

        let result = self
            .connection
            .prepare_cached(
                format!(
                    "INSERT INTO {} (data) VALUES (?1)",
                    ROOT_TABLES[ROOT_OBJECTS]
                )
                .as_str(),
            )?
            .execute(params![slice]);
        expect_change(result, 1)?;
        Ok(self.connection.last_insert_rowid())
    }

    fn get_object(&self, id: i64, builder: capnp::dynamic_value::Builder<'_>) -> Result<()> {
        get_generic::<ROOT_OBJECTS>(&self.connection, id, builder)
    }

    fn drop_object(&self, id: i64) -> Result<()> {
        drop_generic::<ROOT_OBJECTS>(&self.connection, id)
    }

    fn get_string_index(&self, s: &str) -> Result<i64> {
        self.connection
            .prepare_cached("INSERT OR IGNORE INTO stringmap (string) VALUES (?1)")?
            .execute(params![s])?;
        Ok(self
            .connection
            .prepare_cached("SELECT id FROM stringmap WHERE string = ?1")?
            .query_row(params![s], |x| x.get(0))?)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenOptions {
    Create,
    Truncate,
    ReadWrite,
    ReadOnly,
}

fn create(path: impl AsRef<Path>) -> Result<Connection, rusqlite::Error> {
    let span = tracing::span!(tracing::Level::DEBUG, "database::create", path = ?path.as_ref());
    let _enter = span.enter();
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
}

pub fn open_database<DB: DatabaseExt>(
    path: impl AsRef<Path>,
    f: impl Fn(Connection) -> Rc<crate::sqlite_capnp::root::ServerDispatch<DB>>,
    options: OpenOptions,
) -> Result<Rc<crate::sqlite_capnp::root::ServerDispatch<DB>>> {
    let span = tracing::span!(tracing::Level::DEBUG, "database::open_database", path = ?path.as_ref(), options = ?options);
    let _enter = span.enter();
    match options {
        OpenOptions::Create => {
            let create = if let Ok(file) = std::fs::File::open(path.as_ref()) {
                file.metadata()?.len() == 0
            } else {
                true
            };
            let flags = if create {
                OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE
            } else {
                OpenFlags::SQLITE_OPEN_READ_WRITE
            };

            let r = f(Connection::open_with_flags(path, flags)?.into());
            if create {
                r.server.init()?;
            }
            Ok(r)
        }
        OpenOptions::Truncate => {
            // If the file already exists, we truncate it instead of deleting it to support temp file situations.
            if let Ok(file) = std::fs::File::open(path.as_ref()) {
                file.set_len(0)?;
            }
            let r = f(create(path)?);
            r.server.init()?;
            Ok(r)
        }
        OpenOptions::ReadWrite => Ok(f(Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?)),
        OpenOptions::ReadOnly => Ok(f(Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

//type Microseconds = u64; // Microseconds since 1970 (won't overflow for 500000 years)
