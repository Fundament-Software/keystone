capnp_import::capnp_import!(
    "indirect_world.capnp",
    "/hello_world.capnp",
    "/schema/**/*.capnp"
);

pub mod indirect_world;
use crate::indirect_world::IndirectWorldImpl;
use crate::indirect_world_capnp::config;
use crate::indirect_world_capnp::root;
use crate::module_capnp::module_start;
use capnp::any_pointer::Owned as any_pointer;
use capnp_macros::capnproto_rpc;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use std::cell::RefCell;
#[cfg(feature = "tracing")]
use std::fs::File;
#[cfg(feature = "tracing")]
use tracing::Level;

pub struct ModuleImpl {
    bootstrap: RefCell<Option<keystone_capnp::host::Client<any_pointer>>>,
    disconnector: RefCell<Option<capnp_rpc::Disconnector<rpc_twoparty_capnp::Side>>>,
}

#[capnproto_rpc(module_start)]
impl module_start::Server<config::Owned, root::Owned> for ModuleImpl {
    async fn start(&self, config: Reader) -> Result<(), ::capnp::Error> {
        let client: root::Client = capnp_rpc::new_client(IndirectWorldImpl {
            hello_client: config.get_hello_world()?,
        });
        results.get().set_api(client)?;
        Ok(())
    }
    async fn stop(&self) -> Result<(), ::capnp::Error> {
        let r = self.disconnector.borrow_mut().take();
        if let Some(d) = r {
            d.await?;
            Ok(())
        } else {
            Err(capnp::Error::from_kind(capnp::ErrorKind::Disconnected))
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let args: Vec<String> = ::std::env::args().collect();
    tracing::info!("server started");

    #[cfg(feature = "tracing")]
    let log_file = File::create("indirect-world.log")?;
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(log_file)
        .with_ansi(false)
        .init();

    tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move {
                let mut set: capnp_rpc::CapabilityServerSet<
                    ModuleImpl,
                    module_start::Client<crate::indirect_world_capnp::config::Owned, root::Owned>,
                > = capnp_rpc::CapabilityServerSet::new();

                let module_client: module_start::Client<
                    crate::indirect_world_capnp::config::Owned,
                    root::Owned,
                > = set.new_client(ModuleImpl {
                    bootstrap: None.into(),
                    disconnector: None.into(),
                });

                let reader = tokio::io::stdin();
                let writer = tokio::io::stdout();

                let network = twoparty::VatNetwork::new(
                    reader,
                    writer,
                    rpc_twoparty_capnp::Side::Server,
                    Default::default(),
                );
                let mut rpc_system =
                    RpcSystem::new(Box::new(network), Some(module_client.clone().client));

                let server = set.get_local_server_of_resolved(&module_client).unwrap();
                let borrow = server.as_ref();
                *borrow.bootstrap.borrow_mut() =
                    Some(rpc_system.bootstrap(rpc_twoparty_capnp::Side::Client));
                *borrow.disconnector.borrow_mut() = Some(rpc_system.get_disconnector());

                tracing::info!("spawned rpc");
                tokio::task::spawn_local(rpc_system).await.unwrap().unwrap();
            })
            .await
        })
        .await?;

    tracing::info!("RPC gracefully terminated");

    Ok(())
}

#[cfg(test)]
use tempfile::NamedTempFile;

#[test]
fn test_indirect_init() -> eyre::Result<()> {
    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = keystone_util::build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(&keystone_util::build_module_config(
        "Hello World",
        "hello-world-module",
        r#"{ greeting = "Indirect" }"#,
    ));

    source.push_str(&keystone_util::build_module_config(
        "Indirect World",
        "indirect-world-module",
        r#"{ helloWorld = [ "@Hello World" ] }"#,
    ));

    let pool = tokio::task::LocalSet::new();
    let fut = pool.run_until(async move {
        tokio::task::spawn_local(async move {
            let mut instance = keystone::keystone::Keystone::new_from_string(&source)
                .await
                .unwrap();

            {
                let module = &instance.modules[&instance.namemap["Hello World"]];
                let pipe = module
                    .api
                    .as_ref()
                    .ok_or(capnp::Error::failed("api ref didn't exist".to_string()))?
                    .pipeline
                    .get_api()
                    .as_cap();

                let hello_client: crate::hello_world_capnp::root::Client =
                    capnp::capability::FromClientHook::new(pipe);

                let mut sayhello = hello_client.say_hello_request();
                sayhello.get().init_request().set_name("Keystone".into());
                let hello_response = sayhello.send().promise.await?;

                let msg = hello_response.get()?.get_reply()?.get_message()?;

                assert_eq!(msg, "Indirect, Keystone!");
            }

            {
                let module = &instance.modules[&instance.namemap["Indirect World"]];
                let pipe = module
                    .api
                    .as_ref()
                    .ok_or(capnp::Error::failed("api ref didn't exist".to_string()))?
                    .pipeline
                    .get_api()
                    .as_cap();

                let indirect_client: crate::indirect_world_capnp::root::Client =
                    capnp::capability::FromClientHook::new(pipe);

                let mut sayhello = indirect_client.say_hello_request();
                sayhello.get().init_request().set_name("Keystone".into());
                let hello_response = sayhello.send().promise.await?;

                let msg = hello_response.get()?.get_reply()?.get_message()?;

                assert_eq!(msg, "Indirect, Keystone!");
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
