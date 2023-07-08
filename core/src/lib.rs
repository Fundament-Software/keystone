#![allow(dead_code)]
mod couchdb;
mod database;
pub mod http;
mod node;

capnp_import::capnp_import!("core/schema/**/*.capnp");
