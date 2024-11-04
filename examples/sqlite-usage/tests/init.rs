use sqlite_usage::sqlite_usage_capnp::root;

#[test]
fn test_sqlite_usage_init() -> eyre::Result<()> {
    keystone::test_harness(
        &keystone::build_module_config(
            "Sqlite Usage",
            "sqlite-usage-module",
            r#"{ sqlite = [ "@sqlite" ], outer = [ "@keystone", "initCell", {id = "OuterTableRef", default = ["@sqlite", "createTable", { def = [{ name="state", baseType="text", nullable=false }] }, "res"]}, "result" ], inner = [ "@keystone", "initCell", {id = "InnerTableRef"}, "result" ] }"#,
        ),
        |message| async move {
            {
                let mut instance = keystone::test_create_keystone(&message).await.unwrap();
                let usage_client: root::Client = instance.get_api_pipe("Sqlite Usage").unwrap();

                let fut = async move {
                    {
                        let mut echo = usage_client.echo_alphabetical_request();
                        echo.get().init_request().set_name("3 Keystone".into());
                        let echo_response = echo.send().promise.await?;

                        let msg = echo_response.get()?.get_reply()?.get_message()?;

                        assert_eq!(msg, "usage <No Previous Message>");
                    }

                    {
                        let mut echo = usage_client.echo_alphabetical_request();
                        echo.get().init_request().set_name("2 Replace".into());
                        let echo_response = echo.send().promise.await?;

                        let msg = echo_response.get()?.get_reply()?.get_message()?;

                        assert_eq!(msg, "usage 3 Keystone");
                    }

                    {
                        let mut echo = usage_client.echo_alphabetical_request();
                        echo.get().init_request().set_name("1 Reload".into());
                        let echo_response = echo.send().promise.await?;

                        let msg = echo_response.get()?.get_reply()?.get_message()?;

                        assert_eq!(msg, "usage 2 Replace");
                    }
                    Ok::<(), eyre::Error>(())
                };

                tokio::select! {
                    r = keystone::test_runner(&mut instance) => Ok(r?),
                    r = fut => r,
                }?;
                keystone::test_shutdown(&mut instance).await?;
            }

            let mut instance = keystone::test_create_keystone(&message).await.unwrap();
            let usage_client: root::Client = instance.get_api_pipe("Sqlite Usage").unwrap();

            let fut = async move {
                let mut echo = usage_client.echo_alphabetical_request();
                echo.get().init_request().set_name("0 Fin".into());
                let echo_response = echo.send().promise.await?;

                let msg = echo_response.get()?.get_reply()?.get_message()?;

                //panic!("SUCCESS: {:?}", msg);
                assert_eq!(msg, "usage 1 Reload");
                Ok::<(), eyre::Error>(())
            };

            tokio::select! {
                r = keystone::test_runner(&mut instance) => Ok(r?),
                r = fut => r,
            }?;
            keystone::test_shutdown(&mut instance).await
        },
    )
}
