mod harness;
use eyre::Result;
//use harness::test_harness;
use keystone::keystone::Keystone;
use keystone::keystone_capnp::keystone_config;
use tempfile::NamedTempFile;

#[test]
fn test_stateful() -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();

    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = keystone_util::build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(&keystone_util::build_module_config(
        "Stateful",
        "stateful-module",
        r#"{ echoWord = "echo" }"#,
    ));

    keystone::config::to_capnp(&source.parse::<toml::Table>()?, msg.reborrow())?;

    let mut instance = Keystone::new(
        message.get_root_as_reader::<keystone_config::Reader>()?,
        false,
    )?;

    // TODO: might be able to replace the runtime catch below with .unhandled_panic(UnhandledPanic::ShutdownRuntime) if gets stabilized
    let pool = tokio::task::LocalSet::new();
    let fut = pool.run_until(async move {
        tokio::task::spawn_local(async move {
            instance
                .init(message.get_root_as_reader::<keystone_config::Reader>()?)
                .await
                .unwrap();
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
            Ok::<(), capnp::Error>(())
        })
        .await
    });

    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(fut);
    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    result.unwrap().unwrap();

    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();
    keystone::config::to_capnp(&source.parse::<toml::Table>()?, msg.reborrow())?;

    let mut instance = Keystone::new(
        message.get_root_as_reader::<keystone_config::Reader>()?,
        false,
    )?;

    let pool = tokio::task::LocalSet::new();
    let fut = pool.run_until(async move {
        tokio::task::spawn_local(async move {
            instance
                .init(message.get_root_as_reader::<keystone_config::Reader>()?)
                .await
                .unwrap();
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
        })
        .await
    });

    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(fut);
    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    result.unwrap().unwrap();

    Ok(())
}
