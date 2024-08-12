use crate::config_test_capnp::config;
use crate::config_test_capnp::root;
use capnp::private::capability::ClientHook;
use capnp::traits::Imbue;
use capnp::traits::ImbueMut;
use capnp::traits::IntoInternalStructReader;

pub struct ConfigTestImpl {
    pub msg: capnp::message::Builder<capnp::message::HeapAllocator>,
    pub caps: Vec<Option<Box<dyn ClientHook>>>,
}

impl root::Server for ConfigTestImpl {
    async fn get_config(
        &self,
        params: root::GetConfigParams,
        mut results: root::GetConfigResults,
    ) -> Result<(), ::capnp::Error> {
        tracing::debug!("get_config was called! {:?}", self.caps);

        let reader: config::Reader = self.msg.get_root_as_reader()?;
        tracing::debug!("got reader {:?}", reader);
        let mut morecaps = Vec::new();
        results.get().imbue_mut(&mut morecaps);
        results.get().set_reply(reader)?;
        tracing::debug!("set reader");
        Ok(())
    }
}
