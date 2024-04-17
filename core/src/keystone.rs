use crate::keystone_capnp::keystone;
use crate::keystone_capnp::keystone::GetConfigParams;
use crate::keystone_capnp::keystone::GetConfigResults;
use crate::keystone_capnp::keystone::SetConfigParams;
use crate::keystone_capnp::keystone::SetConfigResults;

pub struct KeystoneImpl;

impl keystone::Server for KeystoneImpl {
    async fn get_config(
        &self,
        _: GetConfigParams,
        _: GetConfigResults,
    ) -> Result<(), ::capnp::Error> {
        Ok(())
    }

    async fn set_config(
        &self,
        _: SetConfigParams,
        _: SetConfigResults,
    ) -> Result<(), ::capnp::Error> {
        Ok(())
    }
}
