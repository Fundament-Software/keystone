use std::marker::PhantomData;

use crate::keystone_capnp::host;
use capnp_macros::capnproto_rpc;

pub struct HostImpl<State> {
    instance_id: u64,
    phantom: PhantomData<State>,
}

impl<State> HostImpl<State>
where
    State: ::capnp::traits::Owned,
{
    pub fn new(id: u64) -> Self {
        Self {
            instance_id: id,
            phantom: PhantomData,
        }
    }
}

impl host::Server<capnp::any_pointer::Owned> for HostImpl<capnp::any_pointer::Owned> {}
