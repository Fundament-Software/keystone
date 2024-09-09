use crate::database::DatabaseExt;
use crate::sqlite::SqliteDatabase;
use crate::sqlite_capnp::root::ServerDispatch;
use crate::storage_capnp::sturdy_ref;
use capnp::traits::SetPointerBuilder;
use std::rc::Rc;

pub struct SturdyRefImpl {
    id: i64,
    db: Rc<ServerDispatch<SqliteDatabase>>,
}

impl SturdyRefImpl {
    pub fn new(id: i64, db: Rc<ServerDispatch<SqliteDatabase>>) -> Self {
        Self { id, db }
    }

    pub fn init<R: SetPointerBuilder + Clone>(
        module_id: u64,
        data: R,
        db: Rc<ServerDispatch<SqliteDatabase>>,
    ) -> eyre::Result<Self> {
        let id = db
            .add_sturdyref(module_id, data)
            .map_err(|e| capnp::Error::failed(e.to_string()))?;

        Ok(Self { id, db })
    }

    pub fn get_id(&self) -> i64 {
        self.id
    }
}

impl sturdy_ref::Server<capnp::any_pointer::Owned> for SturdyRefImpl {
    async fn restore(
        &self,
        _: sturdy_ref::RestoreParams<capnp::any_pointer::Owned>,
        mut results: sturdy_ref::RestoreResults<capnp::any_pointer::Owned>,
    ) -> Result<(), ::capnp::Error> {
        let promise = self
            .db
            .get_sturdyref(self.id)
            .map_err(|e| capnp::Error::failed(e.to_string()))?;

        results
            .get()
            .init_cap()
            .set_as_capability(promise.pipeline.get_cap().as_cap());
        Ok(())
    }
}

/*
use capnp::capability::FromClientHook;
use capnp_rpc::CapabilityServerSet;

impl<T: capnp::traits::Owned> sturdy_ref::Server<T> for SturdyRefImpl
where
    for<'a> <T as capnp::traits::Owned>::Reader<'a>: capnp::capability::FromClientHook,
{
    async fn restore(
        &self,
        _: sturdy_ref::RestoreParams<T>,
        mut results: sturdy_ref::RestoreResults<T>,
    ) -> Result<(), ::capnp::Error> {
        let promise = self
            .db
            .get_sturdyref(self.id)
            .map_err(|e| capnp::Error::failed(e.to_string()))?;

        let reader: <T as capnp::traits::Owned>::Reader<'_> =
            capnp::capability::FromClientHook::new(promise.pipeline.get_cap().as_cap());

        results.get().set_cap(reader)?;
        Ok(())
    }
}
*/
