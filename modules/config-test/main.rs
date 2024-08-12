include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

pub mod config_test;
use crate::config_test::ConfigTestImpl;
use crate::config_test_capnp::config;
use crate::config_test_capnp::root;
use capnp::any_pointer::Owned as any_pointer;
use capnp::traits::ImbueMut;
use capnp_macros::capnproto_rpc;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use keystone::module_capnp::module_start;
use std::cell::RefCell;
use tracing::Level;

pub struct ModuleImpl {
    bootstrap: RefCell<Option<keystone::keystone_capnp::host::Client<any_pointer>>>,
    disconnector: RefCell<Option<capnp_rpc::Disconnector<rpc_twoparty_capnp::Side>>>,
}

#[capnproto_rpc(module_start)]
impl module_start::Server<config::Owned, root::Owned> for ModuleImpl {
    async fn start(&self, config: Reader) -> Result<(), ::capnp::Error> {
        tracing::debug!("start()");
        let mut msg = capnp::message::Builder::new_default();
        let mut caps = Vec::new();
        let mut builder: capnp::any_pointer::Builder = msg.init_root();
        builder.imbue_mut(&mut caps);
        builder.set_as(config)?;
        let client: root::Client = capnp_rpc::new_client(ConfigTestImpl { msg, caps });
        results.get().set_api(client)?;
        Ok(())
    }
    async fn stop(&self) -> Result<(), ::capnp::Error> {
        tracing::debug!("stop()");
        let r = self.disconnector.borrow_mut().take();
        if let Some(d) = r {
            d.await?;
            Ok(())
        } else {
            Err(capnp::Error::from_kind(capnp::ErrorKind::Disconnected))
        }
    }
}

async fn init<
    T: tokio::io::AsyncRead + 'static + Unpin,
    U: tokio::io::AsyncWrite + 'static + Unpin,
>(
    reader: T,
    writer: U,
) -> capnp::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .init();

    tracing::info!("Module starting up...");

    tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move {
                let mut set: capnp_rpc::CapabilityServerSet<
                    ModuleImpl,
                    module_start::Client<crate::config_test_capnp::config::Owned, root::Owned>,
                > = capnp_rpc::CapabilityServerSet::new();

                let module_client: module_start::Client<
                    crate::config_test_capnp::config::Owned,
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

                tracing::debug!("spawned rpc");
                tokio::task::spawn_local(rpc_system).await.unwrap().unwrap();
            })
            .await
        })
        .await
        .map_err(|e| capnp::Error::failed(e.to_string()))?;

    tracing::info!("RPC gracefully terminated");

    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _: Vec<String> = ::std::env::args().collect();

    init(tokio::io::stdin(), tokio::io::stdout()).await?;
    Ok(())
}

#[cfg(test)]
use tempfile::NamedTempFile;

#[test]
fn test_complex_config_init() -> eyre::Result<()> {
    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = keystone_util::build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(&keystone_util::build_module_config(
        "Config Test",
        "config-test-module",
        r#"{ nested = { state = [ "@keystone", "initCell", {id = "myCellName"}, "result" ], moreState = [ "@keystone", "initCell", {id = "myCellName"}, "result" ] } }"#,
    ));

    let (client_writer, server_reader) = async_byte_channel::channel();
    let (server_writer, client_reader) = async_byte_channel::channel();

    let pool = tokio::task::LocalSet::new();
    let fut = pool.run_until(async move {
        tokio::task::spawn_local(async move {
            let (mut instance, rpc, disconnect, api) =
                keystone::keystone::Keystone::init_single_module(
                    &source,
                    "Config Test",
                    server_reader,
                    server_writer,
                )
                .await
                .unwrap();

            {
                init(client_reader, client_writer).await?;

                let config_client: crate::config_test_capnp::root::Client = api;

                println!("got api");
                let get_config = config_client.get_config_request();
                let get_response = get_config.send().promise.await?;
                println!("got response");

                let response = get_response.get()?.get_reply()?;
                println!("got reply");
                println!("{:#?}", response);
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
