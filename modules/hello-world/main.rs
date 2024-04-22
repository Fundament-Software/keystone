capnp_import::capnp_import!("hello_world.capnp", "../../core/schema/**/*.capnp");

pub mod hello_world;
use crate::hello_world::HelloWorldImpl;
use async_backtrace;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use std::fs::File;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::Level;

#[tokio::main(flavor = "current_thread")]
#[async_backtrace::framed]
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
        .run_until(async_backtrace::location!().frame(async move {
            let hello_world_client: hello_world_capnp::hello_world::Client =
                capnp_rpc::new_client(HelloWorldImpl);
            let reader = tokio::io::stdin();
            let writer = tokio::io::stdout();

            let network = twoparty::VatNetwork::new(
                reader,
                writer,
                rpc_twoparty_capnp::Side::Server,
                Default::default(),
            );
            let mut rpc_system =
                RpcSystem::new(Box::new(network), Some(hello_world_client.clone().client));

            let bootstrap: keystone_capnp::keystone::Client =
                rpc_system.bootstrap(rpc_twoparty_capnp::Side::Client);
            tracing::info!("spawned rpc");

            tokio::task::spawn_local(async_backtrace::location!().frame(rpc_system))
                .await
                .unwrap()
                .unwrap();
        }))
        .await;

    tracing::error!("should never reach this");

    Ok(())
}