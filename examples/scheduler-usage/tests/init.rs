use scheduler_usage::scheduler_usage_capnp::root;

#[test]
fn test_scheduler_sync() -> eyre::Result<()> {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .init();

    keystone::test_module_harness::<
        scheduler_usage::scheduler_usage_capnp::config::Owned,
        scheduler_usage::scheduler_usage::SchedulerUsageImpl,
        root::Owned,
        _,
    >(
        &keystone::build_module_config(
            "Scheduler Usage",
            "scheduler-usage-module",
            r#"{ greeting = "Bonjour", scheduler = [ "@scheduler" ] }"#,
        ),
        "Scheduler Usage",
        |api| async move {
            let usage_client: scheduler_usage::scheduler_usage_capnp::root::Client = api;
            let mut echo = usage_client.echo_delay_request();
            echo.get().init_request().set_name("Delay".into());
            tracing::info!("Sending echo_delay request");
            let _ = echo.send().promise.await?;

            let r = scheduler_usage::scheduler_usage::MASTER_SYNC
                .1
                .take()
                .unwrap();
            tracing::info!("Awaiting result");
            let result = r.await?;
            tracing::info!("Got result: {}", &result);

            assert_eq!(result, "Bonjour, Delay!");
            Ok::<(), eyre::Error>(())
        },
    )?;

    Ok(())
}
