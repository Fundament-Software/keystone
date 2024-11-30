use eyre::Result;
use stateful::stateful_capnp::root::echo_request::EchoRequest;

#[test]
fn test_stateful() -> Result<()> {
    keystone::test_harness(
        &keystone::build_module_config("Stateful", "stateful-module", r#"{ echoWord = "echo" }"#),
        |message| async move {
            {
                let mut instance = keystone::test_create_keystone(&message).await.unwrap();
                let stateful_client: stateful::stateful_capnp::root::Client =
                    instance.get_api_pipe("Stateful").unwrap();

                let fut = async move {
                    {
                        let echo_response = stateful_client
                            .build_echo_last_request(Some(EchoRequest {
                                _name: "Keystone".into(),
                            }))
                            .send()
                            .promise
                            .await?;

                        let msg = echo_response.get()?.get_reply()?.get_message()?;

                        assert_eq!(msg, "echo ");
                    }

                    {
                        let echo_response = stateful_client
                            .build_echo_last_request(Some(EchoRequest {
                                _name: "Replace".into(),
                            }))
                            .send()
                            .promise
                            .await?;

                        let msg = echo_response.get()?.get_reply()?.get_message()?;

                        assert_eq!(msg, "echo Keystone");
                    }

                    {
                        let echo_response = stateful_client
                            .build_echo_last_request(Some(EchoRequest {
                                _name: "Reload".into(),
                            }))
                            .send()
                            .promise
                            .await?;

                        let msg = echo_response.get()?.get_reply()?.get_message()?;

                        assert_eq!(msg, "echo Replace");
                    }
                    Ok::<(), eyre::Error>(())
                };

                tokio::select! {
                    r = keystone::drive_stream(&mut instance.rpc_systems) => Ok(r?),
                    r = fut => r,
                }?;
                keystone::test_shutdown(&mut instance).await?;
            }

            let mut instance = keystone::test_create_keystone(&message).await.unwrap();
            let stateful_client: stateful::stateful_capnp::root::Client =
                instance.get_api_pipe("Stateful").unwrap();

            let fut = async move {
                let mut echo = stateful_client.echo_last_request();
                echo.get().init_request().set_name("Keystone".into());
                let echo_response = echo.send().promise.await?;

                let msg = echo_response.get()?.get_reply()?.get_message()?;

                assert_eq!(msg, "echo Reload");
                Ok::<(), eyre::Error>(())
            };

            tokio::select! {
                r = keystone::drive_stream(&mut instance.rpc_systems) => Ok(r?),
                r = fut => r,
            }?;
            keystone::test_shutdown(&mut instance).await?;
            Ok::<(), eyre::Error>(())
        },
    )
}
