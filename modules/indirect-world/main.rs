capnp_import::capnp_import!(
    "indirect_world.capnp",
    "../hello-world/hello_world.capnp",
    "../../core/schema/**/*.capnp"
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
use std::fs::File;
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
        if let Some(d) = self.disconnector.borrow_mut().take() {
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

    let log_file = File::create("indirect-world.log")?;
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(log_file)
        .with_ansi(false)
        .init();

    tokio::task::LocalSet::new()
        .run_until(async move {
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
        .await;

    tracing::info!("RPC gracefully terminated");

    Ok(())
}
