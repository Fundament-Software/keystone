use crate::cell::SimpleCellImpl;
use crate::database::RootDatabase;
use crate::keystone::CellCapSet;
use crate::keystone_capnp::host;
use eyre::Result;
use std::{cell::RefCell, marker::PhantomData, rc::Rc};

pub struct HostImpl<State> {
    instance_id: u64,
    db: Rc<RefCell<RootDatabase>>,
    cells: Rc<RefCell<CellCapSet>>,
    phantom: PhantomData<State>,
}

impl<State> HostImpl<State>
where
    State: ::capnp::traits::Owned,
{
    pub fn new(id: u64, db: Rc<RefCell<RootDatabase>>, cells: Rc<RefCell<CellCapSet>>) -> Self {
        Self {
            instance_id: id,
            db,
            cells,
            phantom: PhantomData,
        }
    }
}

impl<State> host::Server<State> for HostImpl<State>
where
    State: ::capnp::traits::Owned,
    for<'a> capnp::dynamic_value::Builder<'a>: From<<State as capnp::traits::Owned>::Builder<'a>>,
{
    async fn get_state(
        &self,
        _: host::GetStateParams<State>,
        mut results: host::GetStateResults<State>,
    ) -> Result<(), ::capnp::Error> {
        self.db
            .borrow_mut()
            .get_state(self.instance_id as i64, results.get().init_state().into())
            .map_err(|e| capnp::Error::failed(e.to_string()))?;

        Ok(())
    }

    async fn set_state(
        &self,
        params: host::SetStateParams<State>,
        _: host::SetStateResults<State>,
    ) -> Result<(), ::capnp::Error> {
        self.db
            .borrow_mut()
            .set_state(self.instance_id as i64, params.get()?.get_state()?)
            .map_err(|e| capnp::Error::failed(e.to_string()))?;
        Ok(())
    }

    async fn init_cell(
        &self,
        params: host::InitCellParams<State>,
        mut results: host::InitCellResults<State>,
    ) -> Result<(), ::capnp::Error> {
        let id = self
            .db
            .borrow_mut()
            .get_string_index(params.get()?.get_id()?.to_str()?)
            .map_err(|e| capnp::Error::failed(e.to_string()))?;

        let client = self
            .cells
            .borrow_mut()
            // Very important to use ::init() here so it gets initialized to a default value
            .new_client(
                SimpleCellImpl::init(id, self.db.clone())
                    .map_err(|e| capnp::Error::failed(e.to_string()))?,
            );

        results.get().set_result(client);
        Ok(())
    }
}
