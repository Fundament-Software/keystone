#![allow(dead_code)]
mod couchdb;
mod database;
pub mod http;
mod node;

include!(concat!(env!("OUT_DIR"), "/capnp_include.rs"));
