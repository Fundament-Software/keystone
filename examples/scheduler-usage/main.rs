include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));
pub mod scheduler_usage;
use crate::scheduler_usage_capnp::root;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    keystone::main::<
        crate::scheduler_usage_capnp::config::Owned,
        scheduler_usage::SchedulerUsageImpl,
        root::Owned,
    >(async move {
        //let _: Vec<String> = ::std::env::args().collect();

        #[cfg(feature = "tracing")]
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .init();
    })
    .await
}
