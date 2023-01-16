pub use crate::http_capnp::{domain as Domain, https as Https, path as Path, path::HttpVerb};

mod https;
pub fn https_client() -> Https::Client {
    capnp_rpc::new_client(https::HttpsImpl::new())
}
// TODO Do we want such convenience methods for domain_client and path_client?
mod domain;

mod path;
