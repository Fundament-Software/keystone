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

            let fut = async move {
                let mut sayhello = hello_client.say_hello_request();
                sayhello.get().init_request().set_name("Keystone".into());
                let hello_response = sayhello.send().promise.await?;

                let msg = hello_response.get()?.get_reply()?.get_message()?;

                assert_eq!(msg, "Bonjour, Keystone!");
                Ok::<(), capnp::Error>(())
            };

            tokio::select! {
                r = keystone::drive_stream(&mut instance.rpc_systems) => Ok(r?),
                r = fut => r,
            }?;
            keystone::test_shutdown(&mut instance).await
        },
    )
}

#[inline]
pub async fn drive_stream_with_error(
    msg: &str,
    stream: &mut futures_util::stream::FuturesUnordered<
        impl std::future::Future<Output = eyre::Result<()>>,
    >,
) {
    use futures_util::StreamExt;
    while let Some(r) = stream.next().await {
        if let Err(e) = r {
            eprintln!("{}: {}", msg, e);
        }
    }
}

#[test]
fn test_hello_world_empty() -> eyre::Result<()> {
    keystone::test_harness(
        &keystone::build_module_config(
            "Hello World",
            "hello-world-module",
            r#"{  greeting = "Bonjour" }"#,
        ),
        |message| async move {
            let mut instance = keystone::test_create_keystone(&message).await.unwrap();

            let (mut shutdown, runner) = instance.shutdown();

            tokio::join!(
                drive_stream_with_error("Error during shutdown!", &mut shutdown),
                drive_stream_with_error("Error during shutdown!", runner)
            );
            Ok::<(), eyre::Report>(())
        },
    )
}
