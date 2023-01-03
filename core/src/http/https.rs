use crate::http_capnp::https as Https;
struct HttpsImpl {}

impl Https::Server for HttpsImpl {
    fn domain(
        &mut self,
        _: Https::DomainParams,
        _: Https::DomainResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        capnp::capability::Promise::err(capnp::Error::unimplemented(
            "method https::Server::domain not implemented".to_string(),
        ))
    }
}
