use crate::database::DatabaseExt;
use crate::sqlite::SqliteDatabase;
use crate::storage_capnp::cell;
use capnp::any_pointer::Owned as any_pointer;
use capnp_macros::capnproto_rpc;
use std::rc::Rc;

pub struct SimpleCellImpl {
    id: i64,
    db: Rc<crate::sqlite_capnp::root::ServerDispatch<SqliteDatabase>>,
}

impl SimpleCellImpl {
    pub fn new(id: i64, db: Rc<crate::sqlite_capnp::root::ServerDispatch<SqliteDatabase>>) -> Self {
        Self { id, db }
    }

    pub fn init(
        id: i64,
        db: Rc<crate::sqlite_capnp::root::ServerDispatch<SqliteDatabase>>,
    ) -> eyre::Result<Self> {
        let empty_struct: u64 = 0b0000000000000000000000000000000011111111111111111111111111111100;
        let segments: &[&[u8]] = &[&empty_struct.to_le_bytes()];
        let segment_array = capnp::message::SegmentArray::new(segments);
        let message = capnp::message::Reader::new(segment_array, Default::default());
        let anypointer: capnp::any_pointer::Reader = message.get_root()?;

        db.server.init_state(id, anypointer)?;
        Ok(Self { id, db })
    }
}

#[capnproto_rpc(cell)]
impl cell::Server<any_pointer> for SimpleCellImpl {
    async fn get(&self) {
        let span = tracing::debug_span!("cell", id = self.id);
        let _enter = span.enter();
        tracing::debug!("get()");
        self.db
            .server
            .get_state(self.id, results.get().init_data().into())
            .map_err(|e| {
                eprintln!("CELL GET ERROR: {}", e);
                capnp::Error::failed(
                    "Cell did not exist! did you forget to set it to something first?".into(),
                )
            })?;
        Ok(())
    }
    async fn set(&self, data: capnp::any_pointer::Reader) {
        let span = tracing::debug_span!("cell", id = self.id);
        let _enter = span.enter();
        tracing::debug!("set()");
        self.db.server.set_state(self.id, data).map_err(|e| {
            eprintln!("CELL SET ERROR: {}", e);
            capnp::Error::failed("Could not set data for cell!".into())
        })?;
        Ok(())
    }
}
