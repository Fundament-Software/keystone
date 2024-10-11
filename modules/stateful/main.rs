include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

pub mod stateful;
use crate::stateful::StatefulImpl;
use crate::stateful_capnp::config;
use crate::stateful_capnp::root;
use capnp::any_pointer::Owned as any_pointer;
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
        let client: root::Client = capnp_rpc::new_client(StatefulImpl {
            echo_word: config.get_echo_word()?.to_string()?,
            echo_last: config.get_state()?,
        });
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let args: Vec<String> = ::std::env::args().collect();

    #[cfg(feature = "tracing")]
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
                    module_start::Client<config::Owned, root::Owned>,
                > = capnp_rpc::CapabilityServerSet::new();

                let module_client: module_start::Client<config::Owned, root::Owned> = set
                    .new_client(ModuleImpl {
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
                rpc_system.await.unwrap();
            })
            .await
        })
        .await?;

    tracing::info!("RPC gracefully terminated");

    Ok(())
}