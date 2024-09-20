use eyre::Result;

#[test]
fn test_stateful() -> Result<()> {
    keystone::test_harness(
        &keystone::build_module_config("Stateful", "stateful-module", r#"{ echoWord = "echo" }"#),
        |message| async move {
            {
                let mut instance = keystone::test_create_keystone(&message).await.unwrap();
                let stateful_client: stateful::stateful_capnp::root::Client =
                    instance.get_api_pipe("Stateful").unwrap();

                {
                    let mut echo = stateful_client.echo_last_request();
                    echo.get().init_request().set_name("Keystone".into());
                    let echo_response = echo.send().promise.await?;

                    let msg = echo_response.get()?.get_reply()?.get_message()?;

                    assert_eq!(msg, "echo ");
                }

                {
                    let mut echo = stateful_client.echo_last_request();
                    echo.get().init_request().set_name("Replace".into());
                    let echo_response = echo.send().promise.await?;

                    let msg = echo_response.get()?.get_reply()?.get_message()?;

                    assert_eq!(msg, "echo Keystone");
                }

                {
                    let mut echo = stateful_client.echo_last_request();
                    echo.get().init_request().set_name("Reload".into());
                    let echo_response = echo.send().promise.await?;

                    let msg = echo_response.get()?.get_reply()?.get_message()?;

                    assert_eq!(msg, "echo Replace");
                }

                instance.shutdown().await;
            }

            let mut instance = keystone::test_create_keystone(&message).await.unwrap();
            let stateful_client: stateful::stateful_capnp::root::Client =
                instance.get_api_pipe("Stateful").unwrap();

            {
                let mut echo = stateful_client.echo_last_request();
                echo.get().init_request().set_name("Keystone".into());
                let echo_response = echo.send().promise.await?;

                let msg = echo_response.get()?.get_reply()?.get_message()?;

                assert_eq!(msg, "echo Reload");
            }

            instance.shutdown().await;
            Ok::<(), capnp::Error>(())
        },
    )
}
