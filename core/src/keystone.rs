use std::marker::PhantomData;

use crate::{cap_std_capnproto::AmbientAuthorityImpl, keystone_capnp::host};
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

pub struct Keystone {
    //db: BackingDB
    //log: caplog::
    file_server: AmbientAuthorityImpl,
}
