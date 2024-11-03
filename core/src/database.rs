use crate::buffer_allocator::BufferAllocator;
use crate::cap_replacement;
use crate::cap_replacement::CapReplacement;
use crate::cap_replacement::GetPointerBuilder;
use crate::sqlite::SqliteDatabase;
use crate::storage_capnp::restore::restore_results;
use crate::keystone::CapnpResult;
use capnp::any_pointer::Owned as any_pointer;
use capnp::capability::FromClientHook;
use capnp::message::{ReaderOptions, TypedReader};
use capnp::private::capability::ClientHook;
use capnp::traits::ImbueMut;
use capnp::traits::SetPointerBuilder;
use eyre::Result;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::rc::Rc;

pub type Microseconds = i64; // Microseconds since 1970 (won't overflow for 200000 years)

/// Internal trait that extends our internal database object with keystone-specific actions
#[allow(dead_code)]
pub trait DatabaseExt {
    fn init(&self) -> Result<()>;

    fn get_generic<const TABLE: usize>(
        &self,
        id: i64,
        builder: capnp::dynamic_value::Builder<'_>,
    ) -> Result<()>;

    fn get_state(
        &self,
        string_index: i64,
        builder: capnp::dynamic_value::Builder<'_>,
    ) -> Result<()>;

    async fn set_state<R: SetPointerBuilder + Clone>(
        &self,
        string_index: i64,
        data: R,
    ) -> Result<()>;

    fn init_state(&self, string_index: i64) -> Result<()>;

    fn get_sturdyref(
        &self,
        sturdy_id: i64,
    ) -> Result<capnp::capability::RemotePromise<restore_results::Owned<any_pointer, any_pointer>>>;
    async fn add_sturdyref<R: SetPointerBuilder + Clone>(
        &self,
        module_id: u64,
        data: R,
        expires: Option<Microseconds>,
    ) -> Result<i64>;
    fn drop_sturdyref(&self, id: i64) -> Result<()>;
    fn clean_sturdyrefs(&self, ids: &[i64]) -> Result<()>;
    async fn add_object<R: SetPointerBuilder + Clone>(&self, data: R) -> Result<i64>;
    fn get_object(&self, id: i64, builder: capnp::dynamic_value::Builder<'_>) -> Result<()>;
    fn drop_object(&self, id: i64) -> Result<()>;
    fn get_string_index(&self, s: &str) -> Result<i64>;
}

// Rust's type system does not like enums in generic constants
const ROOT_STATES: usize = 0;
const ROOT_OBJECTS: usize = 1;
const ROOT_STURDYREFS: &str = "sturdyrefs";
const ROOT_TABLES: &[&str] = &["states", "objects"];

fn expect_change(call: Result<usize, rusqlite::Error>, count: usize) -> Result<()> {
    let n = call?;
    if n != count {
        Err(rusqlite::Error::StatementChangedRows(n).into())
    } else {
        Ok(())
    }
}

trait AllocatorExt: capnp::message::Allocator {
    fn reserve_hint(&mut self, _: usize) {}
}

impl AllocatorExt for BufferAllocator {
    fn reserve_hint(&mut self, n: usize) {
        self.reserve(n);
    }
}

impl AllocatorExt for capnp::message::HeapAllocator {}

async fn get_single_segment<'a, R: SetPointerBuilder + Clone>(
    db: &SqliteDatabase,
    alloc: &'a mut impl AllocatorExt,
    data: R,
) -> capnp::Result<capnp::message::Builder<&'a mut impl AllocatorExt>> {
    let mut message = capnp::message::Builder::new(alloc);

    let mut caps = Vec::new();
    let mut builder: capnp::any_pointer::Builder = message.init_root();
    builder.imbue_mut(&mut caps);
    builder.set_as(data.clone())?;

    if let capnp::OutputSegments::MultiSegment(v) = message.get_segments_for_output() {
        let n = v.into_iter().fold(0, |i, v| i + v.len()) * 2;
        let a = message.into_allocator();
        a.reserve_hint(n);
        message = capnp::message::Builder::new(a);
        caps = Vec::new();
        let mut builder: capnp::any_pointer::Builder = message.init_root();
        builder.imbue_mut(&mut caps);
        builder.set_as(data)?;
    }

    let db_ref = db
        .this
        .upgrade()
        .ok_or(capnp::Error::failed("database doesn't exist".into()))?;
    let mut closure = move |hook: Box<dyn ClientHook>| {
        let db_ref_ref = db_ref.clone();
        async move {
            let saveable: crate::storage_capnp::saveable::Client<capnp::any_pointer::Owned> =
                capnp::capability::FromClientHook::new(hook);

            let response = saveable.save_request().send().promise.await?;
            let sturdyref = capnp::capability::get_resolved_cap(response.get()?.get_ref()?).await;
            let id = db_ref_ref.get_sturdyref_id(sturdyref)?;
            Ok::<i64, capnp::Error>(id)
        }
    };
    let mut builder: capnp::any_pointer::Builder = message.get_root()?;
    // We have to re-imbue with this new builder
    builder.imbue_mut(&mut caps);
    let pointer: GetPointerBuilder = builder.get_as()?;
    cap_replacement::replace(&mut closure, pointer.builder).await?;

    // TODO: return list of inserted IDs
    Ok(message)
}

fn get_segment_slice<'a>(
    message: &'a capnp::message::Builder<&mut impl AllocatorExt>,
) -> capnp::Result<&'a [u8]> {
    let slice = match message.get_segments_for_output() {
        capnp::OutputSegments::SingleSegment(s) => s[0],
        capnp::OutputSegments::MultiSegment(v) => {
            return Err(capnp::Error::from_kind(
                capnp::ErrorKind::InvalidNumberOfSegments(v.len()),
            ));
        }
    };

    Ok(slice)
}

fn drop_generic<const TABLE: usize>(conn: &Connection, id: i64) -> Result<()> {
    expect_change(
        conn.prepare_cached(format!("DELETE FROM {} WHERE id = ?1", ROOT_TABLES[TABLE]).as_str())?
            .execute(params![id]),
        1,
    )
}

fn get_replacement(
    db: &SqliteDatabase,
    index: u64,
    mut builder: capnp::any_pointer::Builder,
) -> capnp::Result<()> {
    let result = db.get_sturdyref(index as i64).to_capnp()?;

    builder.set_as_capability(result.pipeline.get_cap().as_cap());
    Ok(())
}

fn cur_time() -> Microseconds {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .expect("System time set to before 1970?!")
        .as_micros() as Microseconds
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
                    id       INTEGER PRIMARY KEY,
                    module   INTEGER NOT NULL,
                    refcount INTEGER NOT NULL,
                    expires  INTEGER NOT NULL,
                    data     BLOB NOT NULL
                )",
                ROOT_STURDYREFS
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

    fn get_generic<const TABLE: usize>(
        &self,
        id: i64,
        builder: capnp::dynamic_value::Builder<'_>,
    ) -> Result<()> {
        let mut stmt = self.connection.prepare_cached(
            format!("SELECT data FROM {} WHERE id = ?1", ROOT_TABLES[TABLE]).as_str(),
        )?;
        stmt.query_row(
            params![id],
            |row: &rusqlite::Row| -> Result<(), rusqlite::Error> {
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

                let replacement = CapReplacement::new(reader, |value, builder| {
                    get_replacement(self, value, builder)
                });

                match builder {
                    capnp::dynamic_value::Builder::Struct(mut s) => s
                        .copy_from(replacement)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e))),
                    capnp::dynamic_value::Builder::AnyPointer(mut b) => b
                        .set_as(replacement)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e))),
                    _ => Err(rusqlite::Error::InvalidQuery),
                }
            },
        )?;
        Ok(())
    }

    fn get_state(
        &self,
        string_index: i64,
        builder: capnp::dynamic_value::Builder<'_>,
    ) -> Result<()> {
        self.get_generic::<ROOT_STATES>(string_index, builder)
    }

    async fn set_state<R: SetPointerBuilder + Clone>(
        &self,
        string_index: i64,
        data: R,
    ) -> Result<()> {
        fn inner(conn: &rusqlite::Connection, string_index: i64, slice: &[u8]) -> Result<()> {
            let call = conn.prepare_cached(
                format!("INSERT INTO {} (id, data) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET data=?2", ROOT_TABLES[ROOT_STATES]).as_str())?.execute(
                params![string_index, slice]
            );

            expect_change(call, 1)
            // TODO (in this order)
            // Confirm the UPDATE query finished correctly
            // Decrement ref count of all old ids
            // Increment ref count of all new ids
            // Delete any old IDs with refcount of 0
        }

        if let Ok(mut alloc) = self.alloc.try_lock() {
            let message = get_single_segment(self, &mut *alloc, data).await?;
            inner(&self.connection, string_index, get_segment_slice(&message)?)
        } else {
            let mut alloc = capnp::message::HeapAllocator::new();
            let message = get_single_segment(self, &mut alloc, data).await?;
            inner(&self.connection, string_index, get_segment_slice(&message)?)
        }?;

        Ok(())
    }

    fn init_state(&self, string_index: i64) -> Result<()> {
        let slice: &[u8] = &[0; 8];

        let call = self
            .connection
            .prepare_cached(
                format!(
                    "INSERT OR IGNORE INTO {} (id, data) VALUES (?1, ?2)",
                    ROOT_TABLES[ROOT_STATES]
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

    fn get_sturdyref(
        &self,
        id: i64,
    ) -> Result<capnp::capability::RemotePromise<restore_results::Owned<any_pointer, any_pointer>>>
    {
        let mut stmt = self.connection.prepare_cached(
            format!(
                "SELECT data, module FROM {} WHERE id = ?1 AND expires > ?2",
                ROOT_STURDYREFS
            )
            .as_str(),
        )?;
        let promise = stmt.query_row(
            params![id, cur_time()],
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

                let replacement = CapReplacement::new(reader, |value, builder| {
                    get_replacement(self, value, builder)
                });

                let mut req = client.restore_request();
                req.get()
                    .init_data()
                    .set_as(replacement)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                Ok(req.send())
            },
        )?;
        Ok(promise)
    }

    async fn add_sturdyref<R: SetPointerBuilder + Clone>(
        &self,
        module_id: u64,
        data: R,
        expires: Option<Microseconds>,
    ) -> Result<i64> {
        fn inner(
            conn: &rusqlite::Connection,
            module_id: u64,
            expire: i64,
            slice: &[u8],
        ) -> Result<i64> {
            Ok(conn
            .prepare_cached(
                format!(
                    "INSERT INTO {} (module, data, expires, refcount) VALUES (?1, ?2, ?3, 1) RETURNING id",
                    ROOT_STURDYREFS
                )
                .as_str(),
            )?
            .query_row(params![module_id, slice, expire], |row| row.get(0))?)
        }

        let expire = expires.unwrap_or(i64::MAX);
        let result = if let Ok(mut alloc) = self.alloc.try_lock() {
            let message = get_single_segment(self, &mut *alloc, data).await?;
            inner(
                &self.connection,
                module_id,
                expire,
                get_segment_slice(&message)?,
            )
        } else {
            let mut alloc = capnp::message::HeapAllocator::new();
            let message = get_single_segment(self, &mut alloc, data).await?;
            inner(
                &self.connection,
                module_id,
                expire,
                get_segment_slice(&message)?,
            )
        }?;

        Ok(result)
    }

    fn drop_sturdyref(&self, id: i64) -> Result<()> {
        expect_change(
            self.connection
                .prepare_cached(
                    format!(
                        "UPDATE {} SET refcount = refcount - 1 WHERE id = ?1 AND refcount > 0",
                        ROOT_STURDYREFS
                    )
                    .as_str(),
                )?
                .execute(params![id]),
            1,
        )
    }

    fn clean_sturdyrefs(&self, ids: &[i64]) -> Result<()> {
        let list: Vec<String> = ids.iter().map(|id| format!("?{}", id)).collect();

        expect_change(
            self.connection
                .prepare_cached(
                    format!(
                        "DELETE FROM {} WHERE id IN ({}) AND refcount = 0",
                        ROOT_STURDYREFS,
                        list.join(",")
                    )
                    .as_str(),
                )?
                .execute(rusqlite::params_from_iter(ids)),
            1,
        )
    }

    async fn add_object<R: SetPointerBuilder + Clone>(&self, data: R) -> Result<i64> {
        fn inner(conn: &rusqlite::Connection, slice: &[u8]) -> Result<i64> {
            Ok(conn
                .prepare_cached(
                    format!(
                        "INSERT INTO {} (data) VALUES (?1) RETURNING id",
                        ROOT_TABLES[ROOT_OBJECTS]
                    )
                    .as_str(),
                )?
                .query_row(params![slice], |row| row.get(0))?)
        }

        let result = if let Ok(mut alloc) = self.alloc.try_lock() {
            let message = get_single_segment(self, &mut *alloc, data).await?;
            inner(&self.connection, get_segment_slice(&message)?)
        } else {
            let mut alloc = capnp::message::HeapAllocator::new();
            let message = get_single_segment(self, &mut alloc, data).await?;
            inner(&self.connection, get_segment_slice(&message)?)
        }?;

        Ok(result)
    }

    fn get_object(&self, id: i64, builder: capnp::dynamic_value::Builder<'_>) -> Result<()> {
        self.get_generic::<ROOT_OBJECTS>(id, builder)
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
    f: impl Fn(Connection) -> capnp::Result<Rc<crate::sqlite_capnp::root::ServerDispatch<DB>>>,
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

            let r = f(Connection::open_with_flags(path, flags)?)?;
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
            let r = f(create(path)?)?;
            r.server.init()?;
            Ok(r)
        }
        OpenOptions::ReadWrite => Ok(f(Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?)?),
        OpenOptions::ReadOnly => Ok(f(Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?)?),
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
