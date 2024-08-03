use crate::indirect_world_capnp::root;

pub struct IndirectWorldImpl {
    pub hello_client: hello_world::hello_world_capnp::root::Client,
}

impl root::Server for IndirectWorldImpl {
    async fn say_hello(
        &self,
        params: root::SayHelloParams,
        mut results: root::SayHelloResults,
    ) -> Result<(), ::capnp::Error> {
        tracing::debug!("say_hello was called!");
        let request = params.get()?.get_request()?;

        let mut sayhello = self.hello_client.say_hello_request();
        sayhello.get().init_request().set_name(request.get_name()?);
        let hello_response = sayhello.send().promise.await.unwrap();

        let msg = hello_response.get()?.get_reply()?.get_message()?;
        results.get().init_reply().set_message(msg);
        Ok(())
    }
}
