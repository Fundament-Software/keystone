include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

pub mod indirect_world;
use crate::indirect_world_capnp::root;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    keystone::main::<
        crate::indirect_world_capnp::config::Owned,
        indirect_world::IndirectWorldImpl,
        root::Owned,
    >(async move {
        //let _: Vec<String> = ::std::env::args().collect();
    })
    .await
}
