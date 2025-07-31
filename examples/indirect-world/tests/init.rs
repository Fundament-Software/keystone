use indirect_world::indirect_world_capnp::root;

#[test]
fn test_indirect_init() -> eyre::Result<()> {
    let mut source = String::new();

    source.push_str(&keystone::build_module_config(
        "Hello World",
        "hello-world-module",
        r#"{ greeting = "Indirect" }"#,
    ));

    source.push_str(&keystone::build_module_config(
        "Indirect World",
        "indirect-world-module",
        r#"{ helloWorld = [ "@Hello World" ] }"#,
    ));

    keystone::test_module_harness::<
        indirect_world::indirect_world_capnp::config::Owned,
        indirect_world::indirect_world::IndirectWorldImpl,
        root::Owned,
        _,
    >(&source, "Indirect World", |api| async move {
        let indirect_client: indirect_world::indirect_world_capnp::root::Client = api;
        let mut sayhello = indirect_client.say_hello_request();
        sayhello.get().init_request().set_name("Keystone".into());
        let hello_response = sayhello.send().promise.await?;

        let msg = hello_response.get()?.get_reply()?.get_message()?;

        assert_eq!(msg, "Indirect, Keystone!");
        Ok::<(), eyre::Error>(())
    })?;

    Ok(())
}
