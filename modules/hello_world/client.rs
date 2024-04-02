use crate::hello_world_capnp::hello_world;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use std::net::ToSocketAddrs;
use futures::AsyncReadExt;

pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = ::std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: {} client MESSAGE", args[0]);
        return Ok(());
    }
    eprintln!("client started");

    let msg = args[2].to_string();

    tokio::task::LocalSet::new()
        .run_until(async move {
           let reader = tokio::io::stdin();
           let reader = tokio_util::compat::TokioAsyncReadCompatExt::compat(reader);
           let writer = tokio::io::stdout();
           let writer = tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(writer);
           let network = twoparty::VatNetwork::new(
                reader,
                writer,
                rpc_twoparty_capnp::Side::Client,
                Default::default(),
           );
           let mut rpc_system = RpcSystem::new(Box::new(network), None);
           let hello_world: hello_world::Client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);

           tokio::task::spawn_local(rpc_system);
           eprintln!("rpc_system spawned");

           let mut request = hello_world.say_hello_request();
           request.get().init_request().set_name(msg[..].into());

           eprintln!("request constructed");
           let reply = request.send().promise.await?;
           eprintln!("reply sent and acquired: {}", reply.get()?.get_reply()?.get_message()?.to_str()?);
           Ok(())
        })
        .await
}
