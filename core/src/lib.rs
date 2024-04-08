#![allow(dead_code)]
mod config;
pub mod http;

capnp_import::capnp_import!("schema/**/*.capnp");
