pub mod http_capnp {
    include!(concat!(env!("OUT_DIR"), "/capnp_include.rs"));
}

mod https;

mod domain;

mod path;
