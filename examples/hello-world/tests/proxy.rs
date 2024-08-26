use capnp::private::capability::ClientHook;
use keystone::ProxyServer;

#[test]
fn test_hello_world_proxy() -> eyre::Result<()> {
    keystone::test_harness(
        &keystone::build_module_config(
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
