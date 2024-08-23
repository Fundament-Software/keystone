include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

use crate::hello_world_capnp::root;
use capnp::any_pointer::Owned as any_pointer;

pub struct HelloWorldImpl {
    pub greeting: String,
}

impl root::Server for HelloWorldImpl {
    async fn say_hello(
        &self,
        params: root::SayHelloParams,
        mut results: root::SayHelloResults,
    ) -> Result<(), ::capnp::Error> {
        tracing::debug!("say_hello was called!");
        let request = params.get()?.get_request()?;
        let name = request.get_name()?.to_str()?;
        let greet = self.greeting.as_str();
        let message = format!("{greet}, {name}!");

        results.get().init_reply().set_message(message[..].into());
        Ok(())
    }
}

impl keystone::Module<hello_world_capnp::config::Owned> for HelloWorldImpl {
    async fn new(
        config: <hello_world_capnp::config::Owned as capnp::traits::Owned>::Reader<'_>,
        _: keystone::keystone_capnp::host::Client<any_pointer>,
    ) -> capnp::Result<Self> {
        Ok(HelloWorldImpl {
            greeting: config.get_greeting()?.to_string()?,
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    keystone::main::<crate::hello_world_capnp::config::Owned, HelloWorldImpl, root::Owned>(
        async move {
            //let _: Vec<String> = ::std::env::args().collect();

            #[cfg(feature = "tracing")]
            tracing_subscriber::fmt()
                .with_max_level(Level::DEBUG)
                .with_writer(std::io::stderr)
                .with_ansi(true)
                .init();
        },
    )
    .await
}
