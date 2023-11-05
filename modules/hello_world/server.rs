use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};

use crate::hello_world_capnp::hello_world;
use crate::hello_world_impl::HelloWorldImpl;

use futures::AsyncReadExt;
use futures::AsyncBufReadExt;
use futures::io::BufReader;
use tokio::time::{sleep, Duration};
use log::{debug, error, log_enabled, info, Level};

pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("server started");

    tokio::task::LocalSet::new()
        .run_until(async move {
            let hello_world_client: hello_world::Client = capnp_rpc::new_client(HelloWorldImpl);
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
            let rpc_system = RpcSystem::new(Box::new(network), Some(hello_world_client.clone().client));

            tokio::task::spawn_local(rpc_system);
            eprintln!("spawned rpc");

            eprintln!("looping");
            loop {
                sleep(Duration::from_millis(100)).await;
            }
            eprintln!("should never reach this");
        })
        .await
}
