use crate::config_test_capnp::config;
use crate::config_test_capnp::root;

pub struct ConfigTestImpl {
    pub msg: capnp::message::Builder<capnp::message::HeapAllocator>,
}

impl root::Server for ConfigTestImpl {
    async fn get_config(
        &self,
        params: root::GetConfigParams,
        mut results: root::GetConfigResults,
    ) -> Result<(), ::capnp::Error> {
        tracing::info!("get_config was called!");
        let reader: config::Reader = self.msg.get_root_as_reader()?;
        results.get().set_reply(reader)?;
        Ok(())
    }
}
