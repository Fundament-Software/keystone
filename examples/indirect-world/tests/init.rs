use indirect_world::indirect_world_capnp::root;
use tempfile::NamedTempFile;

#[test]
fn test_indirect_init() -> eyre::Result<()> {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .init();

    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = keystone::build_temp_config(&temp_db, &temp_log, &temp_prefix);

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

    let (client_writer, server_reader) = async_byte_channel::channel();
    let (server_writer, client_reader) = async_byte_channel::channel();

    let pool = tokio::task::LocalSet::new();
    let a = pool.run_until(pool.spawn_local(keystone::start::<
        indirect_world::indirect_world_capnp::config::Owned,
        indirect_world::indirect_world::IndirectWorldImpl,
        root::Owned,
        async_byte_channel::Receiver,
        async_byte_channel::Sender,
    >(client_reader, client_writer)));

    let b = pool.run_until(pool.spawn_local(async move {
        let (mut instance, rpc, _disconnect, api) = keystone::Keystone::init_single_module(
            &source,
            "Indirect World",
            server_reader,
            server_writer,
        )
        .await
        .unwrap();

        let handle = tokio::task::spawn_local(rpc);
        let indirect_client: indirect_world::indirect_world_capnp::root::Client = api;

        {
            let mut sayhello = indirect_client.say_hello_request();
            sayhello.get().init_request().set_name("Keystone".into());
            let hello_response = sayhello.send().promise.await?;

            let msg = hello_response.get()?.get_reply()?.get_message()?;

            assert_eq!(msg, "Indirect, Keystone!");
        }

        tokio::select! {
            r = handle => r,
            _ = instance.shutdown() => Ok(Ok(())),
        }
        .unwrap()
        .unwrap();

        Ok::<(), capnp::Error>(())
    }));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async move {
        tokio::select! {
            r = a => r,
            r = b => r,
            r = tokio::signal::ctrl_c() => Ok(Ok(r.expect("failed to capture ctrl-c"))),
        }
    });

    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    result.unwrap().unwrap();

    Ok(())
}
