include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

pub mod indirect_world;
use crate::indirect_world::IndirectWorldImpl;
use crate::indirect_world_capnp::config;
use crate::indirect_world_capnp::root;
use capnp::any_pointer::Owned as any_pointer;
use capnp_macros::capnproto_rpc;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use keystone::module_capnp::module_start;
use std::cell::RefCell;

#[cfg(feature = "tracing")]
use tracing::Level;

pub struct ModuleImpl {
    bootstrap: RefCell<Option<keystone::keystone_capnp::host::Client<any_pointer>>>,
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

async fn init<
    T: tokio::io::AsyncRead + 'static + Unpin,
    U: tokio::io::AsyncWrite + 'static + Unpin,
>(
    reader: T,
    writer: U,
) -> capnp::Result<()> {
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

    let network = twoparty::VatNetwork::new(
        reader,
        writer,
        rpc_twoparty_capnp::Side::Server,
        Default::default(),
    );
    let mut rpc_system = RpcSystem::new(Box::new(network), Some(module_client.clone().client));

    let server = set.get_local_server_of_resolved(&module_client).unwrap();
    let borrow = server.as_ref();
    *borrow.bootstrap.borrow_mut() = Some(rpc_system.bootstrap(rpc_twoparty_capnp::Side::Client));
    *borrow.disconnector.borrow_mut() = Some(rpc_system.get_disconnector());

    tracing::debug!("spawned rpc");
    rpc_system
        .await
        .map_err(|e| capnp::Error::failed(e.to_string()))?;

    tracing::debug!("rpc callback returned");
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _: Vec<String> = ::std::env::args().collect();

    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .init();

    tracing::info!("Module starting up...");

    tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(init(tokio::io::stdin(), tokio::io::stdout())).await
        })
        .await??;

    tracing::info!("RPC gracefully terminated");
    Ok(())
}

#[cfg(test)]
use tempfile::NamedTempFile;

#[test]
fn test_indirect_init() -> eyre::Result<()> {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .init();

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

    let (client_writer, server_reader) = async_byte_channel::channel();
    let (server_writer, client_reader) = async_byte_channel::channel();

    let pool = tokio::task::LocalSet::new();
    let a = pool.run_until(pool.spawn_local(init(client_reader, client_writer)));

    let b = pool.run_until(pool.spawn_local(async move {
        let (mut instance, rpc, _disconnect, api) =
            keystone::keystone::Keystone::init_single_module(
                &source,
                "Indirect World",
                server_reader,
                server_writer,
            )
            .await
            .unwrap();

        let handle = tokio::task::spawn_local(rpc);
        let indirect_client: crate::indirect_world_capnp::root::Client = api;

        {
            let mut sayhello = indirect_client.say_hello_request();
            sayhello.get().init_request().set_name("Keystone".into());
            let hello_response = sayhello.send().promise.await?;

            let msg = hello_response.get()?.get_reply()?.get_message()?;

            assert_eq!(msg, "Indirect, Keystone!");
        }

        tokio::select! {
            r = handle => r,
            _ = instance.shutdown() => Ok(Ok(())),
        }
        .unwrap()
        .unwrap();

        Ok::<(), capnp::Error>(())
    }));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async move {
        tokio::select! {
            r = a => r,
            r = b => r,
            r = tokio::signal::ctrl_c() => Ok(Ok(r.expect("failed to capture ctrl-c"))),
        }
    });

    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    result.unwrap().unwrap();

    Ok(())
}
