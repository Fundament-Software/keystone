mod harness;
use eyre::Result;
use harness::test_harness;

#[test]
fn test_hello_world_init() -> Result<()> {
    test_harness(
        &keystone_util::build_module_config(
            "Hello World",
            "hello-world-module",
            r#"{  greeting = "Bonjour" }"#,
        ),
        |mut instance| async move {
            let hello_client: hello_world::hello_world_capnp::root::Client =
                instance.get_api_pipe("Hello World").unwrap();

            let mut sayhello = hello_client.say_hello_request();
            sayhello.get().init_request().set_name("Keystone".into());
            let hello_response = sayhello.send().promise.await?;

            let msg = hello_response.get()?.get_reply()?.get_message()?;

            assert_eq!(msg, "Bonjour, Keystone!");

            instance.shutdown().await;
            Ok(())
        },
    )
}

use capnp::private::capability::ClientHook;
use keystone::keystone::ProxyServer;

#[test]
fn test_hello_world_proxy() -> Result<()> {
    test_harness(
        &keystone_util::build_module_config(
            "Hello World",
            "hello-world-module",
            r#"{  greeting = "Bonjour" }"#,
        ),
        |mut instance| async move {
            let module = &instance.modules[&instance.namemap["Hello World"]];
            let pipe = instance
                .proxy_set
                .borrow_mut()
                .new_client(ProxyServer::new(
                    module.queue.add_ref(),
                    instance.proxy_set.clone(),
                ))
                .hook;

            let hello_client: hello_world::hello_world_capnp::root::Client =
                capnp::capability::FromClientHook::new(pipe);

            let mut sayhello = hello_client.say_hello_request();
            sayhello.get().init_request().set_name("Keystone".into());
            let hello_response = sayhello.send().promise.await?;

            let msg = hello_response.get()?.get_reply()?.get_message()?;

            assert_eq!(msg, "Bonjour, Keystone!");

            instance.shutdown().await;
            Ok(())
        },
    )
}
