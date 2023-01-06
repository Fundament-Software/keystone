use crate::http_capnp::domain as Domain;
use crate::http_capnp::https as Https;
use capnp::capability::Promise;
pub struct DomainImpl {
    https_cap: Https::Client,
}

impl DomainImpl {
    pub fn new(https_cap: Https::Client) -> Self {
        DomainImpl { https_cap }
    }
}

impl Domain::Server for DomainImpl {
    fn subdomain(
        &mut self,
        _: Domain::SubdomainParams,
        _: Domain::SubdomainResults,
    ) -> Promise<(), capnp::Error> {
        Promise::err(capnp::Error::unimplemented(
            "method domain::Server::subdomain not implemented".to_string(),
        ))
    }

    fn path(&mut self, _: Domain::PathParams, _: Domain::PathResults) -> Promise<(), capnp::Error> {
        Promise::err(capnp::Error::unimplemented(
            "method domain::Server::path not implemented".to_string(),
        ))
    }
}
