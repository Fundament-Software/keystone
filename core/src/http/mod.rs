pub mod http_capnp {
    include!(concat!(env!("OUT_DIR"), "/capnp_include.rs"));
}

mod domain;
mod https;
mod path;
//mod response;
