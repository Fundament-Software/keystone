use crate::http_capnp::domain as Domain;
struct DomainImpl;

impl Domain::Server for DomainImpl {
    fn subdomain(
        &mut self,
        _: Domain::SubdomainParams,
        _: Domain::SubdomainResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        capnp::capability::Promise::err(capnp::Error::unimplemented(
            "method domain::Server::subdomain not implemented".to_string(),
        ))
    }

    fn path(
        &mut self,
        _: Domain::PathParams,
        _: Domain::PathResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        capnp::capability::Promise::err(capnp::Error::unimplemented(
            "method domain::Server::path not implemented".to_string(),
        ))
    }
}
