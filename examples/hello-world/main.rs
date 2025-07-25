use capnp_macros::capnproto_rpc;
use hello_world::hello_world_capnp;
use hello_world::hello_world_capnp::root;
use keystone::capnp::any_pointer::Owned as any_pointer;
use keystone::{capnp, capnp_rpc, tokio};
use std::rc::Rc;

pub struct HelloWorldImpl {
    pub greeting: String,
}

#[capnproto_rpc(root)]
impl root::Server for HelloWorldImpl {
    async fn say_hello(self: Rc<Self>, request: Reader) -> capnp::Result<Self> {
        tracing::debug!("say_hello was called!");
        let name = request.get_name()?.to_str()?;
        let greet = self.greeting.as_str();
        let message = format!("{greet}, {name}!");
        results.get().init_reply().set_message(message[..].into());
        Ok(())
    }
    async fn get_an_int(self: Rc<Self>) -> capnp::Result<Self> {
        results.get().set(8);
        return Ok(());
    }
    async fn echo(self: Rc<Self>, i: i8) -> capnp::Result<Self> {
        results.get().set(i as u16);
        return Ok(());
    }
    async fn int(self: Rc<Self>) -> capnp::Result<Self> {
        results.get().set(capnp_rpc::new_client(HelloWorldImpl {
            greeting: "".to_string(),
        }));
        return Ok(());
    }
    async fn str(self: Rc<Self>, request: Reader) -> capnp::Result<Self> {
        results.get().set(request.get_name()?.to_str()?);
        return Ok(());
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
        },
    )
    .await
}

#[test]
fn test_hello_world_inline() -> eyre::Result<()> {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(true)
        .init();

    keystone::test_module_harness::<
        crate::hello_world_capnp::config::Owned,
        HelloWorldImpl,
        crate::hello_world_capnp::root::Owned,
        _,
    >(
        &keystone::build_module_config(
            "Hello World",
            "hello-world-module",
            r#"{  greeting = "Bonjour" }"#,
        ),
        "Hello World",
        |api| async move {
            let hello_client: crate::hello_world_capnp::root::Client = api;

            let mut sayhello = hello_client.say_hello_request();
            sayhello.get().init_request().set_name("Keystone".into());
            let hello_response = sayhello.send().promise.await?;

            let msg = hello_response.get()?.get_reply()?.get_message()?;

            assert_eq!(msg, "Bonjour, Keystone!");
            Ok::<(), eyre::Report>(())
        },
    )?;

    Ok(())
}
