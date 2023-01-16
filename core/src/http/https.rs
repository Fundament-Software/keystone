use super::domain::DomainImpl;
use super::{Domain, Https};
use capnp::capability::Promise;
use capnp_rpc::pry;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;

pub struct HttpsImpl {
    https_client: hyper::Client<HttpsConnector<HttpConnector>>,
}

impl HttpsImpl {
    pub fn new() -> Self {
        let connector = HttpsConnector::new();
        let https_client = hyper::Client::builder().build::<_, hyper::Body>(connector);
        HttpsImpl { https_client }
    }
}

impl Https::Server for HttpsImpl {
    fn domain(
        &mut self,
        params: Https::DomainParams,
        mut results: Https::DomainResults,
    ) -> Promise<(), capnp::Error> {
        let domain_name = pry!(pry!(params.get()).get_name());
        let domain_impl = DomainImpl::new(domain_name, self.https_client.clone());
        if let Err(e) = domain_impl {
            return Promise::err(e);
        }
        let domain: Domain::Client = capnp_rpc::new_client(domain_impl.unwrap());
        results.get().set_result(domain);
        Promise::ok(())
    }
}
