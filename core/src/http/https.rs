use std::borrow::Borrow;

use crate::http_capnp::domain as Domain;
use crate::http_capnp::https as Https;
use capnp::capability::Promise;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;

struct HttpsImpl {
    https_client: hyper::Client<HttpsConnector<HttpConnector>>,
}

impl HttpsImpl {
    fn new() -> Self {
        let connector = HttpsConnector::new();
        let https_client = hyper::Client::builder().build::<_, hyper::Body>(connector);
        HttpsImpl { https_client }
    }
}

impl Https::Server for HttpsImpl {
    fn domain(
        &mut self,
        _: Https::DomainParams,
        mut results: Https::DomainResults,
    ) -> Promise<(), capnp::Error> {
        let https_client: Https::Client = capnp_rpc::new_client(HttpsImpl::new()); // TODO figure out how to pass its own cap
        let domain: Domain::Client =
            capnp_rpc::new_client(crate::http::domain::DomainImpl::new(https_client));
        results.get().set_domain(domain);
        Promise::ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session() {
        let url = "https://www.example.com";
        let https: Https::Client = capnp_rpc::new_client(HttpsImpl::new());
        let request = https.domain_request();
        let domain = request.send().pipeline.get_domain();
        let mut request = domain.path_request();
        request.get().set_name("")
    }
}
