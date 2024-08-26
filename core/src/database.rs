use crate::buffer_allocator::BufferAllocator;
use crate::sqlite::SqliteDatabase;
use capnp::message::{ReaderOptions, TypedReader};
use capnp::traits::SetPointerBuilder;
use eyre::Result;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
        builder: capnp::dynamic_value::Builder<'_>,
    ) -> Result<()>;
    fn add_sturdyref<R: SetPointerBuilder + Clone>(&self, data: R) -> Result<i64>;
    fn set_sturdyref<R: SetPointerBuilder + Clone>(&self, sturdy_id: i64, data: R) -> Result<()>;
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
const ROOT_STRINGMAP: usize = 3;
const ROOT_TABLES: &[&str] = &["states", "sturdyrefs", "objects", "stringmap"];

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

            let message_reader =
                TypedReader::<_, capnp::any_pointer::Owned>::new(capnp::message::Reader::new(
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

/*fn get_single_segment<'a, R: SetPointerBuilder + Clone>(
    message: &'a mut capnp::message::Builder<&mut BufferAllocator>,
    data: R,
) -> Result<&'a [u8]> {
    message.set_root(data)?;
    if let capnp::OutputSegments::MultiSegment(v) = message.get_segments_for_output() {
        let n = v.into_iter().fold(0, |i, v| i + v.len()) * 2;
        std::mem::swap(x, y)
        let a = (*message).into_allocator();
        a.reserve(n);
        *message = capnp::message::Builder::new(a);
        message.set_root(data)?;
    }

    match message.get_segments_for_output() {
        capnp::OutputSegments::SingleSegment(s) => Ok(s[0]),
        capnp::OutputSegments::MultiSegment(v) => Err(capnp::Error::from_kind(
            capnp::ErrorKind::InvalidNumberOfSegments(v.len()),
        )
        .into()),
    }
}*/

fn set_generic<const TABLE: usize, const UPDATE: bool, R: SetPointerBuilder + Clone>(
    conn: &Connection,
    mut alloc: std::cell::RefMut<BufferAllocator>,
    string_index: i64,
    data: R,
) -> Result<()> {
    let mut message = capnp::message::Builder::new(&mut *alloc);
    message.set_root(data.clone())?;

    if let capnp::OutputSegments::MultiSegment(v) = message.get_segments_for_output() {
        let n = v.into_iter().fold(0, |i, v| i + v.len()) * 2;
        let a = message.into_allocator(); // it is very important the old message is deallocated
        a.reserve(n);
        message = capnp::message::Builder::new(a);
        message.set_root(data)?;
    }

    let slice = match message.get_segments_for_output() {
        capnp::OutputSegments::SingleSegment(s) => s[0],
        capnp::OutputSegments::MultiSegment(v) => {
            return Err(
                capnp::Error::from_kind(capnp::ErrorKind::InvalidNumberOfSegments(v.len())).into(),
            );
        }
    };
    //let slice = Self::get_single_segment(&mut message, data)?;

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
    let mut message = capnp::message::Builder::new(&mut *alloc);
    message.set_root(data.clone())?;

    if let capnp::OutputSegments::MultiSegment(v) = message.get_segments_for_output() {
        let n = v.into_iter().fold(0, |i, v| i + v.len()) * 2;
        let a = message.into_allocator();
        a.reserve(n);
        message = capnp::message::Builder::new(a);
        message.set_root(data)?;
    }

    let slice = match message.get_segments_for_output() {
        capnp::OutputSegments::SingleSegment(s) => s[0],
        capnp::OutputSegments::MultiSegment(v) => {
            return Err(
                capnp::Error::from_kind(capnp::ErrorKind::InvalidNumberOfSegments(v.len())).into(),
            );
        }
    };

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
        for t in [ROOT_STATES, ROOT_STURDYREFS, ROOT_OBJECTS] {
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
        sturdy_id: i64,
        builder: capnp::dynamic_value::Builder<'_>,
    ) -> Result<()> {
        get_generic::<ROOT_STURDYREFS>(&self.connection, sturdy_id, builder)
    }
    fn add_sturdyref<R: SetPointerBuilder + Clone>(&self, data: R) -> Result<i64> {
        add_generic::<ROOT_STURDYREFS, R>(&self.connection, self.alloc.borrow_mut(), data)
    }
    fn set_sturdyref<R: SetPointerBuilder + Clone>(&self, sturdy_id: i64, data: R) -> Result<()> {
        set_generic::<ROOT_STURDYREFS, true, R>(
            &self.connection,
            self.alloc.borrow_mut(),
            sturdy_id,
            data,
        )
    }
    fn drop_sturdyref(&self, id: i64) -> Result<()> {
        drop_generic::<ROOT_STURDYREFS>(&self.connection, id)
    }
    fn add_object<R: SetPointerBuilder + Clone>(&self, data: R) -> Result<i64> {
        add_generic::<ROOT_OBJECTS, R>(&self.connection, self.alloc.borrow_mut(), data)
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

pub fn open_database<DB: From<Connection> + DatabaseExt>(
    path: impl AsRef<Path>,
    options: OpenOptions,
) -> Result<DB> {
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

            let r: DB = Connection::open_with_flags(path, flags)?.into();
            if create {
                r.init()?;
            }
            Ok(r)
        }
        OpenOptions::Truncate => {
            // If the file already exists, we truncate it instead of deleting it to support temp file situations.
            if let Ok(file) = std::fs::File::open(path.as_ref()) {
                file.set_len(0)?;
            }
            let r: DB = create(path)?.into();
            r.init()?;
            Ok(r)
        }
        OpenOptions::ReadWrite => {
            Ok(Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?.into())
        }
        OpenOptions::ReadOnly => {
            Ok(Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?.into())
        }
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

type Microseconds = u64; // Microseconds since 1970 (won't overflow for 500000 years)
