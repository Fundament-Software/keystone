use crate::database::DatabaseExt;
use crate::sqlite::SqliteDatabase;
use crate::sqlite_capnp::root::ServerDispatch;
use crate::storage_capnp::cell;
use capnp::any_pointer::Owned as any_pointer;
use capnp_macros::capnproto_rpc;
use std::rc::Rc;

pub struct SimpleCellImpl {
    id: i64,
    db: Rc<SqliteDatabase>,
}

impl SimpleCellImpl {
    pub fn new(id: i64, db: Rc<SqliteDatabase>) -> Self {
        Self { id, db }
    }

    pub fn init(id: i64, db: Rc<SqliteDatabase>) -> eyre::Result<Self> {
        // We can't initialize to an empty struct because Cells might not be structs, and we sometimes need to know if we set the cell before.
        //let empty_struct: u64 = 0b0000000000000000000000000000000011111111111111111111111111111100;
        //let segments: &[&[u8]] = &[&empty_struct.to_le_bytes()];
        //let segment_array = capnp::message::SegmentArray::new(segments);
        //let message = capnp::message::Reader::new(segment_array, Default::default());
        //let anypointer: capnp::any_pointer::Reader = message.get_root()?;

        db.init_state(id)?;
        Ok(Self { id, db })
    }
}

#[capnproto_rpc(cell)]
impl cell::Server<any_pointer> for SimpleCellImpl {
    async fn get(self: Rc<Self>) {
        let span = tracing::debug_span!("cell", id = self.id);
        let _enter = span.enter();
        tracing::debug!("get()");
        let mut builder = results.get().init_data();
        self.db
            .get_state(self.id, builder.reborrow().into())
            .map_err(|e| {
                eprintln!("CELL GET ERROR: {}", e);
                capnp::Error::failed(
                    "Cell did not exist! did you forget to set it to something first?".into(),
                )
            })?;
        if builder.is_null() {
            builder.clear();
        }
        Ok(())
    }
    async fn set(self: Rc<Self>, data: capnp::any_pointer::Reader) {
        // TODO: Cleanup sturdyrefs that end up getting deleted by this operation

        let span = tracing::debug_span!("cell", id = self.id);
        let _enter = span.enter();
        tracing::debug!("set()");

        self.db.set_state(self.id, data).await.map_err(|e| {
            eprintln!("CELL SET ERROR: {}", e);
            capnp::Error::failed("Could not set data for cell!".into())
        })?;

        Ok(())
    }
}
