use super::path::PathImpl;
use super::{Domain, Path};
use capnp::capability::Promise;
use capnp_rpc::pry;

pub struct DomainImpl {
    domain_name: String, // TODO Should be some sort of URI type that only gets domain
                         // In hyper: `use hyper::http::uri::Authority;`
                         // Quite possibly a list of domains that will be combined in path
                         // TODO The full domain name may not exceed a total length of 253 ASCII characters
}

impl DomainImpl {
    pub fn new<S: Into<String>>(domain_name: S) -> Self {
        DomainImpl {
            domain_name: domain_name.into(),
        }
    }
}

impl Domain::Server for DomainImpl {
    fn subdomain(
        &mut self,
        params: Domain::SubdomainParams,
        mut results: Domain::SubdomainResults,
    ) -> Promise<(), capnp::Error> {
        let original_domain_name = self.domain_name.clone();
        let name = pry!(pry!(params.get()).get_name());
        let new_domain_name = name.to_string() + "." + &original_domain_name;
        let domain: Domain::Client = capnp_rpc::new_client(DomainImpl::new(new_domain_name));
        results.get().set_result(domain);
        Promise::ok(())
    }

    fn path(
        &mut self,
        params: Domain::PathParams,
        mut results: Domain::PathResults,
    ) -> Promise<(), capnp::Error> {
        let name = pry!(pry!(params.get()).get_name());
        let path_impl = PathImpl::new(self.domain_name.as_str(), name);
        if let Err(e) = path_impl {
            return Promise::err(e);
        }
        let path_impl = path_impl.unwrap();
        let path: Path::Client = capnp_rpc::new_client(path_impl);
        results.get().set_result(path);
        Promise::ok(())
    }
}
