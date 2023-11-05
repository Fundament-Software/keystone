pub mod hello_world_capnp {
    include!(concat!(env!("OUT_DIR"), "/hello_world_capnp.rs"));
}
pub mod client;
pub mod server;
pub mod hello_world_impl;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = ::std::env::args().collect();
    if args.len() >= 1 {
        match &args[1][..] {
            "client" => return client::main().await,
            "server" => return server::main().await,
            _ => (),
        }
    }
    println!("usage: {} [client | server]", args[0]);
    Ok(())
}
