use eyre::Result;

#[test]
fn test_complex_config_init() -> Result<()> {
    keystone::test_harness(
        &keystone::build_module_config(
            "Complex Config",
            "complex-config-module",
            r#"{ nested = { state = [ "@keystone", "initCell", {id = "myCellName"}, "result" ], moreState = [ "@keystone", "initCell", {id = "myCellName"}, "result" ] } }"#,
        ),
        |message| async move {
            let (mut instance, mut rpc_systems) =
                keystone::test_create_keystone(&message).await.unwrap();
            let config_client: complex_config::complex_config_capnp::root::Client =
                instance.get_api_pipe("Complex Config").unwrap();

            let fut = async move {
                let get_config = config_client.get_config_request();
                let get_response = get_config.send().promise.await?;

                let _ = get_response.get()?.get_reply()?;
                Ok::<(), capnp::Error>(())
            };

            tokio::select! {
                r = keystone::drive_stream(&mut rpc_systems) => Ok(r?),
                r = fut => r,
            }?;
            keystone::test_shutdown(&mut instance, &mut rpc_systems).await
        },
    )
}
