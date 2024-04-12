capnp_import::capnp_import!("hello_world.capnp");

pub mod hello_world;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};

use crate::hello_world::HelloWorldImpl;
use tokio::time::{sleep, Duration};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = ::std::env::args().collect();
    tracing::info!("server started");

    tokio::task::LocalSet::new()
        .run_until(async move {
            let hello_world_client: hello_world_capnp::hello_world::Client =
                capnp_rpc::new_client(HelloWorldImpl);
            let reader = tokio::io::stdin();
            let reader = tokio_util::compat::TokioAsyncReadCompatExt::compat(reader);
            let writer = tokio::io::stdout();
            let writer = tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(writer);

            let network = twoparty::VatNetwork::new(
                reader,
                writer,
                rpc_twoparty_capnp::Side::Server,
                Default::default(),
            );
            let rpc_system =
                RpcSystem::new(Box::new(network), Some(hello_world_client.clone().client));

            tokio::task::spawn_local(rpc_system);
            tracing::info!("spawned rpc");

            tracing::info!("looping");
            loop {
                sleep(Duration::from_millis(100)).await;
            }
            tracing::error!("should never reach this");
        })
        .await
}
