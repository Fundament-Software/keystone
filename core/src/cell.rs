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

#[capnproto_rpc(cell)]
impl cell::Server<any_pointer> for SimpleCellImpl {
    async fn get(&self) {
        self.db
            .borrow_mut()
            .get_state(self.id, results.get().init_data())
            .map_err(|_| capnp::Error::from_kind(capnp::ErrorKind::Failed))?;
        Ok(())
    }
    async fn set(&self, data: capnp::any_pointer::Reader) {
        self.db
            .borrow_mut()
            .set_state(self.id, data)
            .map_err(|_| capnp::Error::from_kind(capnp::ErrorKind::Failed))?;
        Ok(())
    }
}
