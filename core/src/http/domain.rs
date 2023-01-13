use super::path::PathImpl;
use super::{Domain, Path};
use capnp::capability::Promise;
use capnp_rpc::pry;
use hyper::http::uri::Authority;

pub struct DomainImpl {
    domain_name: Authority,
}

impl DomainImpl {
    pub fn new<A: TryInto<Authority>>(domain_name: A) -> Result<Self, capnp::Error> {
        Ok(DomainImpl {
            domain_name: domain_name.try_into().map_err(|_| {
                capnp::Error::failed("Can't create domain - invalid authority".to_string())
            })?,
        })
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
        let new_domain_name = name.to_string() + "." + original_domain_name.as_str();
        let domain_impl = DomainImpl::new(new_domain_name);
        if let Err(e) = domain_impl {
            return Promise::err(e);
        }
        let domain: Domain::Client = capnp_rpc::new_client(domain_impl.unwrap());
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
        let path: Path::Client = capnp_rpc::new_client(path_impl.unwrap());
        results.get().set_result(path);
        Promise::ok(())
    }
}
