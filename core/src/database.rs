use capnp::message::{ReaderOptions, TypedReader};
use eyre::{eyre, Result};
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub trait DatabaseInterface {
    fn init(&mut self) -> Result<()>;
}

pub struct RootDatabase {
    conn: Connection,
    buf: Vec<u8>,
}

// Rust's type system does not like enums in generic constants
const ROOT_STATES: usize = 0;
const ROOT_STURDYREFS: usize = 1;
const ROOT_OBJECTS: usize = 2;
const ROOT_STRINGMAP: usize = 3;
const ROOT_TABLES: &'static [&'static str] = &["states", "sturdyrefs", "objects", "stringmap"];

impl RootDatabase {
    fn expect_change(call: Result<usize, rusqlite::Error>, count: usize) -> Result<()> {
        if call? != count {
            Err(eyre!("Didn't actually insert a row?!"))
        } else {
            Ok(())
        }
    }
    fn get_generic<const TABLE: usize>(
        &mut self,
        id: i64,
        builder: &mut capnp::any_pointer::Builder<'_>,
    ) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            format!("SELECT data FROM {} WHERE id = ?1", ROOT_TABLES[TABLE]).as_str(),
        )?;
        self.buf = stmt.query_row(
            params![id],
            |row: &rusqlite::Row| -> Result<Vec<u8>, rusqlite::Error> { row.get(0) },
        )?;
        let mut slice = self.buf.as_slice();

        let message_reader = TypedReader::<_, capnp::any_pointer::Owned>::new(
            capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut slice,
                ReaderOptions {
                    traversal_limit_in_words: None,
                    nesting_limit: 128,
                },
            )?,
        );
        builder.set_as(message_reader.get()?)?;
        Ok(())
    }

    fn set_generic<const TABLE: usize, R: capnp::message::ReaderSegments>(
        &mut self,
        string_index: i64,
        data: &R,
    ) -> Result<()> {
        capnp::serialize::write_message_segments(&mut self.buf, data)?;
        let result = self.conn.prepare_cached(
            format!("INSERT INTO {} (id, data) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET data=?2", ROOT_TABLES[TABLE]).as_str())?.execute(
            params![string_index, self.buf.as_slice()]
        );
        self.buf.clear();
        Self::expect_change(result, 1)
    }

    fn add_generic<const TABLE: usize, R: capnp::message::ReaderSegments>(
        &mut self,
        data: &R,
    ) -> Result<i64> {
        capnp::serialize::write_message_segments(&mut self.buf, data)?;
        let result = self
            .conn
            .prepare_cached(
                format!("INSERT INTO {} (data) VALUES (?1)", ROOT_TABLES[TABLE]).as_str(),
            )?
            .execute(params![self.buf.as_slice()]);
        self.buf.clear();
        Self::expect_change(result, 1)?;
        Ok(self.conn.last_insert_rowid())
    }

    fn drop_generic<const TABLE: usize>(&mut self, id: i64) -> Result<()> {
        Self::expect_change(
            self.conn
                .prepare_cached(
                    format!("DELETE FROM {} WHERE id = ?1", ROOT_TABLES[TABLE]).as_str(),
                )?
                .execute(params![id]),
            1,
        )
    }

    pub fn get_state(
        &mut self,
        string_index: i64,
        builder: &mut capnp::any_pointer::Builder<'_>,
    ) -> Result<()> {
        self.get_generic::<ROOT_STATES>(string_index, builder)
    }

    pub fn set_state<R: capnp::message::ReaderSegments>(
        &mut self,
        string_index: i64,
        data: &R,
    ) -> Result<()> {
        self.set_generic::<ROOT_STATES, R>(string_index, data)
    }

    pub fn get_sturdyref(
        &mut self,
        sturdy_id: i64,
        builder: &mut capnp::any_pointer::Builder<'_>,
    ) -> Result<()> {
        self.get_generic::<ROOT_STURDYREFS>(sturdy_id, builder)
    }
    pub fn add_sturdyref<R: capnp::message::ReaderSegments>(&mut self, data: &R) -> Result<i64> {
        self.add_generic::<ROOT_STURDYREFS, R>(data)
    }
    pub fn set_sturdyref<R: capnp::message::ReaderSegments>(
        &mut self,
        sturdy_id: i64,
        data: &R,
    ) -> Result<()> {
        self.set_generic::<ROOT_STURDYREFS, R>(sturdy_id, data)
    }
    pub fn drop_sturdyref(&mut self, id: i64) -> Result<()> {
        self.drop_generic::<ROOT_STURDYREFS>(id)
    }
    pub fn add_object<R: capnp::message::ReaderSegments>(&mut self, data: &R) -> Result<i64> {
        self.add_generic::<ROOT_OBJECTS, R>(data)
    }
    pub fn get_object(
        &mut self,
        id: i64,
        builder: &mut capnp::any_pointer::Builder<'_>,
    ) -> Result<()> {
        self.get_generic::<ROOT_OBJECTS>(id, builder)
    }
    pub fn drop_object(&mut self, id: i64) -> Result<()> {
        self.drop_generic::<ROOT_OBJECTS>(id)
    }
    pub fn get_string_index(&mut self, s: &str) -> Result<i64> {
        Self::expect_change(
            self.conn
                .prepare_cached("INSERT OR IGNORE INTO stringmap (string) VALUES (?1)")?
                .execute(params![s]),
            1,
        )?;
        Ok(self.conn.last_insert_rowid())
    }
}

impl From<Connection> for RootDatabase {
    fn from(conn: Connection) -> Self {
        Self {
            conn,
            buf: Vec::new(),
        }
    }
}

impl DatabaseInterface for RootDatabase {
    fn init(&mut self) -> Result<()> {
        // These three tables look the same but we interact with them slightly differently
        for t in [ROOT_STATES, ROOT_STURDYREFS, ROOT_OBJECTS] {
            self.conn.execute(
                format!(
                    "CREATE TABLE {} (
                        id    INTEGER PRIMARY KEY,
                        data  BLOB NOT NULL
                    )",
                    ROOT_TABLES[t as usize]
                )
                .as_str(),
                (),
            )?;
        }

        self.conn.execute(
            "CREATE TABLE stringmap (
            id    INTEGER PRIMARY KEY,
            string  TEXT UNIQUE NOT NULL,
        )",
            (),
        )?;

        Ok(())
    }
}
pub struct Manager {}

pub enum OpenOptions {
    Create,
    Truncate,
    ReadWrite,
    ReadOnly,
}

impl Manager {
    fn create(path: &Path) -> Result<Connection, rusqlite::Error> {
        Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
    }

    pub fn open_database<DB: From<Connection> + DatabaseInterface>(
        path: &Path,
        options: OpenOptions,
    ) -> Result<DB> {
        match options {
            OpenOptions::Create => {
                if let Ok(r) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
                {
                    Ok(r.into())
                } else {
                    let mut r: DB = Self::create(path)?.into();
                    r.init()?;
                    Ok(r)
                }
            }
            OpenOptions::Truncate => {
                std::fs::remove_file(path)?;
                let mut r: DB = Self::create(path)?.into();
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
}

#[derive(Debug, Serialize, Deserialize)]
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
