include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

pub mod complex_config;
use crate::complex_config::ComplexConfigImpl;
use crate::complex_config_capnp::config;
use crate::complex_config_capnp::root;
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
        let client: root::Client = capnp_rpc::new_client(ComplexConfigImpl { msg, caps });
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
    let mut set: capnp_rpc::CapabilityServerSet<
        ModuleImpl,
        module_start::Client<crate::complex_config_capnp::config::Owned, root::Owned>,
    > = capnp_rpc::CapabilityServerSet::new();

    let module_client: module_start::Client<
        crate::complex_config_capnp::config::Owned,
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
fn test_complex_config_init() -> eyre::Result<()> {
    //console_subscriber::init();

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
        "Complex Config",
        "complex-config-module",
        r#"{ nested = { state = [ "@keystone", "initCell", {id = "myCellName"}, "result" ], moreState = [ "@keystone", "initCell", {id = "myCellName"}, "result" ] } }"#,
    ));

    let (client_writer, server_reader) = async_byte_channel::channel();
    let (server_writer, client_reader) = async_byte_channel::channel();

    let pool = tokio::task::LocalSet::new();
    let a = pool.run_until(pool.spawn_local(init(client_reader, client_writer)));

    let b = pool.run_until(pool.spawn_local(async move {
        let (mut instance, rpc, _disconnect, api) =
            keystone::keystone::Keystone::init_single_module(
                &source,
                "Complex Config",
                server_reader,
                server_writer,
            )
            .await
            .unwrap();

        let handle = tokio::task::spawn_local(rpc);
        let config_client: crate::complex_config_capnp::root::Client = api;

        tracing::debug!("Got API");
        let get_config = config_client.get_config_request();
        let get_response = get_config.send().promise.await?;
        tracing::debug!("Got Response");

        let response = get_response.get()?.get_reply()?;
        tracing::debug!("Got Reply");
        println!("{:#?}", response);

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
            r = tokio::signal::ctrl_c() => {
                    /*let handle = tokio::runtime::Handle::current();
                    if let Ok(dump) = tokio::time::timeout(tokio::time::Duration::from_secs(2), handle.dump()).await {
                        for (i, task) in dump.tasks().iter().enumerate() {
                            let trace = task.trace();
                            println!("TASK {i}:");
                            println!("{trace}\n");
                        }
                }*/
                eprintln!("Ctrl-C detected, aborting!");
                Ok(Ok(r.expect("failed to capture ctrl-c")))
            },
        }
    });

    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    result.unwrap().unwrap();

    Ok(())
}
