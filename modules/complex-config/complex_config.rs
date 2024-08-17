use crate::complex_config_capnp::config;
use crate::complex_config_capnp::root;
use capnp::private::capability::ClientHook;
use capnp::traits::Imbue;
use capnp::traits::ImbueMut;

pub struct ComplexConfigImpl {
    pub msg: capnp::message::Builder<capnp::message::HeapAllocator>,
    pub caps: Vec<Option<Box<dyn ClientHook>>>,
}

impl root::Server for ComplexConfigImpl {
    async fn get_config(
        &self,
        _params: root::GetConfigParams,
        mut results: root::GetConfigResults,
    ) -> Result<(), ::capnp::Error> {
        tracing::debug!("get_config was called! {:?}", self.caps);

        let mut reader: config::Reader = self.msg.get_root_as_reader()?;
        // You MUST re-imbue a message with it's cap table every time you get a new reader for it!
        // Yes this is completely insane.
        reader.imbue(&self.caps);
        let mut morecaps = Vec::new();
        results.get().imbue_mut(&mut morecaps);
        results.get().set_reply(reader)?;
        Ok(())
    }
}
