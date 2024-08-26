include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

pub mod stateful;
use crate::stateful::StatefulImpl;
use crate::stateful_capnp::root;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    keystone::main::<crate::stateful_capnp::config::Owned, StatefulImpl, root::Owned>(async move {
        //let _: Vec<String> = ::std::env::args().collect();

        #[cfg(feature = "tracing")]
        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .init();
    })
    .await
}
