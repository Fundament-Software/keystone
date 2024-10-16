use hello_world::hello_world_capnp::root;

#[test]
fn test_hello_world_init() -> eyre::Result<()> {
    /*
    use std::path::PathBuf;
    use std::str::FromStr;

    let binpath = test_binary::TestBinary::relative_to_parent(
        "hello-world-module",
        &PathBuf::from_str("Cargo.toml")?,
    )
    .build()
    .expect("error building test binary");*/

    keystone::test_harness(
        &keystone::build_module_config(
            "Hello World",
            "hello-world-module",
            r#"{  greeting = "Bonjour" }"#,
        ),
        |message| async move {
            let mut instance = keystone::test_create_keystone(&message).await.unwrap();
            let hello_client: root::Client = instance.get_api_pipe("Hello World").unwrap();

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
