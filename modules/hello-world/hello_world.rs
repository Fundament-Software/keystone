use crate::hello_world_capnp::hello_world;
pub struct HelloWorldImpl;

impl hello_world::Server for HelloWorldImpl {
    #[async_backtrace::framed]
    async fn say_hello(
        &self,
        params: hello_world::SayHelloParams,
        mut results: hello_world::SayHelloResults,
    ) -> Result<(), ::capnp::Error> {
        tracing::info!("say_hello was called!");
        let request = params.get()?.get_request()?;
        let name = request.get_name()?.to_str()?;
        let message = format!("Hello, {name}!");

        results.get().init_reply().set_message(message[..].into());
        Ok(())
    }
}
