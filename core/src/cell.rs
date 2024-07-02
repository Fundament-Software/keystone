use crate::database::RootDatabase;
use crate::storage_capnp::cell;
use capnp::any_pointer::Owned as any_pointer;
use capnp_macros::capnproto_rpc;
use std::cell::RefCell;
use std::rc::Rc;

pub struct SimpleCellImpl {
    id: i64,
    db: Rc<RefCell<RootDatabase>>,
}

impl SimpleCellImpl {
    pub fn new(id: i64, db: Rc<RefCell<RootDatabase>>) -> Self {
        Self { id, db }
    }

    pub fn init(id: i64, db: Rc<RefCell<RootDatabase>>) -> eyre::Result<Self> {
        let empty_struct: u64 = 0b0000000000000000000000000000000011111111111111111111111111111100;
        let segments: &[&[u8]] = &[&empty_struct.to_le_bytes()];
        let segment_array = capnp::message::SegmentArray::new(segments);
        let message = capnp::message::Reader::new(segment_array, Default::default());
        let anypointer: capnp::any_pointer::Reader = message.get_root()?;

        db.borrow_mut().init_state(id, anypointer)?;
        Ok(Self { id, db })
    }
}

#[capnproto_rpc(cell)]
impl cell::Server<any_pointer> for SimpleCellImpl {
    async fn get(&self) {
        self.db
            .borrow_mut()
            .get_state(self.id, results.get().init_data())
            .map_err(|e| {
                println!("ERROR: {}", e.to_string());
                capnp::Error::failed(
                    "Cell did not exist! did you forget to set it to something first?".into(),
                )
            })?;
        Ok(())
    }
    async fn set(&self, data: capnp::any_pointer::Reader) {
        self.db.borrow_mut().set_state(self.id, data).map_err(|e| {
            println!("ERROR: {}", e.to_string());
            capnp::Error::failed("Could not set data for cell!".into())
        })?;
        Ok(())
    }
}
