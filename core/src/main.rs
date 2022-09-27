use couch_rs::types::find::FindQuery;
use log::{error, info, warn, LevelFilter};
use std::env;
use std::error::Error;
extern crate pretty_env_logger;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::convert::Infallible;
use std::net::SocketAddr;

const DB_HOST: &str = "http://localhost:5984";
const TEST_DB: &str = "test_db";
mod example_capnp {
    include!(concat!(env!("OUT_DIR"), "/schema/example_capnp.rs"));
}

async fn hello_world(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new("Hello, World".into()))
}

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen to shutdown signal");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    pretty_env_logger::formatted_builder()
        .format_timestamp(None)
        .target(pretty_env_logger::env_logger::fmt::Target::Stdout)
        .init();

    /*let client = couch_rs::Client::new(DB_HOST, "admin", "password")?;

    let db = client.db(TEST_DB).await?;
    let find_all = FindQuery::find_all();
    let docs = db.find_raw(&find_all).await?;*/

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));

    // A `Service` is needed for every connection, so this
    // creates one from our `hello_world` function.
    let make_svc = make_service_fn(|_conn| async {
        // service_fn converts our function into a `Service`
        Ok::<_, Infallible>(service_fn(hello_world))
    });

    let server = Server::bind(&addr).serve(make_svc);

    let graceful = server.with_graceful_shutdown(shutdown_signal());

    // Run this server for... forever!
    if let Err(e) = graceful.await {
        eprintln!("server error: {}", e);
    }

    println!("Performing graceful shutdown...");
    Ok(())
}
