pub mod http_capnp {
    include!(concat!(env!("OUT_DIR"), "/capnp_include.rs"));
}

pub use crate::http_capnp::{domain as Domain, https as Https, path as Path};

mod https;
pub fn https_client() -> Https::Client {
    capnp_rpc::new_client(https::HttpsImpl)
}
// TODO Do we want such convenience methods for domain_client and path_client?
mod domain;

mod path;
