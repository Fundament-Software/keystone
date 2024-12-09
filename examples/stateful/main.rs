include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

pub mod stateful;
use crate::stateful::StatefulImpl;
use crate::stateful_capnp::root;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    keystone::main::<crate::stateful_capnp::config::Owned, StatefulImpl, root::Owned>(async move {
        //let _: Vec<String> = ::std::env::args().collect();
    })
    .await
}
