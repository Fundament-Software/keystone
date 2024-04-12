use crate::keystone_capnp::keystone;
use crate::keystone_capnp::keystone::GetConfigParams;
use crate::keystone_capnp::keystone::GetConfigResults;
use crate::keystone_capnp::keystone::SetConfigParams;
use crate::keystone_capnp::keystone::SetConfigResults;

pub struct KeystoneImpl;

impl keystone::Server for KeystoneImpl {
    fn get_config<'a, 'b>(
        &'a mut self,
        _: GetConfigParams,
        _: GetConfigResults,
    ) -> Result<impl std::future::Future<Output = Result<(), ::capnp::Error>> + 'b, ::capnp::Error>
    {
        capnp::ok()
    }

    fn set_config<'a, 'b>(
        &'a mut self,
        _: SetConfigParams,
        _: SetConfigResults,
    ) -> Result<impl std::future::Future<Output = Result<(), ::capnp::Error>> + 'b, ::capnp::Error>
    {
        capnp::ok()
    }
}
