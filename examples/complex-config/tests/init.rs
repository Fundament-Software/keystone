use eyre::Result;

#[test]
fn test_complex_config_init() -> Result<()> {
    keystone::test_harness(
        &keystone::build_module_config(
            "Complex Config",
            "complex-config-module",
            r#"{ nested = { state = [ "@keystone", "initCell", {id = "myCellName"}, "result" ], moreState = [ "@keystone", "initCell", {id = "myCellName"}, "result" ] } }"#,
        ),
        |mut instance| async move {
            let config_client: complex_config::complex_config_capnp::root::Client =
                instance.get_api_pipe("Complex Config").unwrap();

            println!("got api");
            let get_config = config_client.get_config_request();
            let get_response = get_config.send().promise.await?;
            println!("got response");

            let response = get_response.get()?.get_reply()?;
            println!("got reply");
            println!("{:#?}", response);

            instance.shutdown().await;
            Ok(())
        },
    )
}
