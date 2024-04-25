capnp_import::capnp_import!("hello_world.capnp", "../../core/schema/**/*.capnp");

pub mod hello_world;
use crate::hello_world::HelloWorldImpl;
use crate::hello_world_capnp::config;
use crate::hello_world_capnp::root;
use crate::module_capnp::module_start;
use capnp::any_pointer::Owned as any_pointer;
use capnp_macros::capnproto_rpc;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use std::fs::File;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::Level;

pub struct ModuleImpl;

impl module_start::Server<config::Owned, any_pointer, root::Owned> for ModuleImpl {
    async fn start(
        &self,
        params: module_start::StartParams<config::Owned, any_pointer, root::Owned>,
        mut result: module_start::StartResults<config::Owned, any_pointer, root::Owned>,
    ) -> Result<(), ::capnp::Error> {
        let client: root::Client = capnp_rpc::new_client(HelloWorldImpl {
            greeting: params.get()?.get_config()?.get_greeting()?.to_string()?,
        });
        result.get().set_api(client)?;
        Ok(())
    }
    async fn stop(
        &self,
        _: module_start::StopParams<config::Owned, any_pointer, root::Owned>,
        _: module_start::StopResults<config::Owned, any_pointer, root::Owned>,
    ) -> Result<(), ::capnp::Error> {
        Result::<(), capnp::Error>::Err(::capnp::Error::unimplemented(
            "method module_start::Server::stop not implemented".to_string(),
        ))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = ::std::env::args().collect();
    tracing::info!("server started");

    let log_file = File::create("my_cool_trace.log")?;
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(log_file)
        .with_ansi(false)
        .init();

    tokio::task::LocalSet::new()
        .run_until(async move {
            let module_client: module_start::Client<
                crate::hello_world_capnp::config::Owned,
                any_pointer,
                root::Owned,
            > = capnp_rpc::new_client(ModuleImpl);
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

            let bootstrap: keystone_capnp::host::Client<any_pointer> =
                rpc_system.bootstrap(rpc_twoparty_capnp::Side::Client);
            tracing::info!("spawned rpc");

            tokio::task::spawn_local(rpc_system).await.unwrap().unwrap();
        })
        .await;

    tracing::error!("should never reach this");

    Ok(())
}
