use capnp::capability::Promise;
use crate::hello_world_capnp::hello_world;
pub struct HelloWorldImpl;

use capnp_rpc::pry;

impl hello_world::Server for HelloWorldImpl {
    fn say_hello(
        &mut self,
        params: hello_world::SayHelloParams,
        mut results: hello_world::SayHelloResults,
    ) -> Promise<(), ::capnp::Error> {
        eprintln!("HelloWorldImpl say_hello was called!");
        let request = pry!(pry!(params.get()).get_request());
        let name = pry!(pry!(request.get_name()).to_str());
        let message = format!("Hello, {name}!");

        results.get().init_reply().set_message(message[..].into());

        Promise::ok(())
    }
}
