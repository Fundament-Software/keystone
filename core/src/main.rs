#![allow(dead_code)]
mod couchdb;
mod database;
mod node;

include!(concat!(env!("OUT_DIR"), "/capnp_include.rs"));

use eyre::Result;
use std::env;
extern crate pretty_env_logger;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::convert::Infallible;
use std::net::SocketAddr;

use crate::example_capnp::person;

const DB_HOST: &str = "http://localhost:5984";

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
async fn main() -> Result<()> {
    // Setup eyre
    color_eyre::install()?;

    pretty_env_logger::formatted_builder()
        .format_timestamp(None)
        .target(pretty_env_logger::env_logger::fmt::Target::Stdout)
        .init();

    let b = std::time::SystemTime::now();

    let db = couchdb::CouchDB::new(DB_HOST, "fundament", "hunter2", "SYSLOG").await?;
    let node = node::Node::new(0, &db, &db, "KEYSTONE");

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
