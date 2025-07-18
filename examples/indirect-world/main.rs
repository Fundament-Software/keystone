use indirect_world::indirect_world_capnp::root;
use keystone::tokio;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    keystone::main::<
        indirect_world::indirect_world_capnp::config::Owned,
        indirect_world::indirect_world::IndirectWorldImpl,
        root::Owned,
    >(async move {
        //let _: Vec<String> = ::std::env::args().collect();
    })
    .await
}
