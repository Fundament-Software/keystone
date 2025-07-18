use scheduler_usage::scheduler_usage_capnp::root;

use keystone::tokio;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    keystone::main::<
        scheduler_usage::scheduler_usage_capnp::config::Owned,
        scheduler_usage::scheduler_usage::SchedulerUsageImpl,
        root::Owned,
    >(async move {
        //let _: Vec<String> = ::std::env::args().collect();
    })
    .await
}
