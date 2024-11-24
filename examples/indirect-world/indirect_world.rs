use crate::indirect_world_capnp::root;
use capnp::any_pointer::Owned as any_pointer;
use capnp_macros::capnproto_rpc;

pub struct IndirectWorldImpl {
    pub hello_client: hello_world::hello_world_capnp::root::Client,
}

#[capnproto_rpc(root)]
impl root::Server for IndirectWorldImpl {
    async fn say_hello(&self, request: Reader) -> Result<(), ::capnp::Error> {
        tracing::debug!("say_hello was called!");

        let mut sayhello = self.hello_client.say_hello_request();
        sayhello.get().init_request().set_name(request.get_name()?);
        let hello_response = sayhello.send().promise.await.unwrap();

        let msg = hello_response.get()?.get_reply()?.get_message()?;
        results.get().init_reply().set_message(msg);
        Ok(())
    }
}

impl keystone::Module<crate::indirect_world_capnp::config::Owned> for IndirectWorldImpl {
    async fn new(
        config: <crate::indirect_world_capnp::config::Owned as capnp::traits::Owned>::Reader<'_>,
        _: keystone::keystone_capnp::host::Client<any_pointer>,
    ) -> capnp::Result<Self> {
        Ok(IndirectWorldImpl {
            hello_client: config.get_hello_world()?,
        })
    }
}
