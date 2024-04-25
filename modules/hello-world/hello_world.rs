use crate::hello_world_capnp::root;
pub struct HelloWorldImpl {
    pub greeting: String,
}

impl root::Server for HelloWorldImpl {
    async fn say_hello(
        &self,
        params: root::SayHelloParams,
        mut results: root::SayHelloResults,
    ) -> Result<(), ::capnp::Error> {
        tracing::info!("say_hello was called!");
        let request = params.get()?.get_request()?;
        let name = request.get_name()?.to_str()?;
        let greet = self.greeting.as_str();
        let message = format!("{greet}, {name}!");

        results.get().init_reply().set_message(message[..].into());
        Ok(())
    }
}
