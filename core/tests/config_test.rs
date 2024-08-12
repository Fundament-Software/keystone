mod harness;
use eyre::Result;
use harness::attach_trace;
use harness::test_harness;

#[test]
fn test_complex_config_init() -> Result<()> {
    attach_trace();
    test_harness(
        &keystone_util::build_module_config(
            "Config Test",
            "config-test-module",
            r#"{ nested = { state = [ "@keystone", "initCell", {id = "myCellName"}, "result" ], moreState = [ "@keystone", "initCell", {id = "myCellName"}, "result" ] } }"#,
        ),
        |mut instance| async move {
            let config_client: config_test::config_test_capnp::root::Client =
                instance.get_api_pipe("Config Test").unwrap();

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
