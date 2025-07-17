use keystone::tokio;
use stateful::stateful::StatefulImpl;
use stateful::stateful_capnp::root;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    keystone::main::<stateful::stateful_capnp::config::Owned, StatefulImpl, root::Owned>(
        async move {
            //let _: Vec<String> = ::std::env::args().collect();
        },
    )
    .await
}
