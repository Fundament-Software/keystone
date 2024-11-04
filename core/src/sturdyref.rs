use crate::database::DatabaseExt;
use crate::keystone::CapnpResult;
use crate::sqlite::SqliteDatabase;
use crate::sqlite_capnp::root::ServerDispatch;
use crate::storage_capnp::sturdy_ref;
use capnp::any_pointer::Owned as any_pointer;
use capnp::traits::SetPointerBuilder;
use std::rc::Rc;

pub struct SturdyRefImpl {
    id: i64,
    db: Rc<ServerDispatch<SqliteDatabase>>,
}

impl SturdyRefImpl {
    pub async fn init<'a, R: SetPointerBuilder + Clone>(
        module_id: u64,
        data: R,
        db: Rc<ServerDispatch<SqliteDatabase>>,
    ) -> eyre::Result<Self> {
        let id = db.add_sturdyref(module_id, data, None).await.to_capnp()?;

        Ok(Self { id, db })
    }

    pub fn get_id(&self) -> i64 {
        self.id
    }
}

impl sturdy_ref::Server<any_pointer> for SturdyRefImpl {
    async fn restore(
        &self,
        _: sturdy_ref::RestoreParams<any_pointer>,
        mut results: sturdy_ref::RestoreResults<any_pointer>,
    ) -> Result<(), ::capnp::Error> {
        let promise = self.db.get_sturdyref(self.id).to_capnp()?;

        results
            .get()
            .init_cap()
            .set_as_capability(promise.pipeline.get_cap().as_cap());
        Ok(())
    }
}

impl Drop for SturdyRefImpl {
    fn drop(&mut self) {
        if let Err(e) = self.db.drop_sturdyref(self.id) {
            // We can't allow a failure here to crash the program, so we do nothing
            eprintln!("Failed to drop SturdyRef! {}", e);
        }
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
            .to_capnp()?;

        let reader: <T as capnp::traits::Owned>::Reader<'_> =
            capnp::capability::FromClientHook::new(promise.pipeline.get_cap().as_cap());

        results.get().set_cap(reader)?;
        Ok(())
    }
}
*/
