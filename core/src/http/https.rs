use super::domain::DomainImpl;
use super::{Domain, Https};
use capnp::capability::Promise;
use capnp_macros::capnproto_rpc;
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
#[capnproto_rpc(Https)]
impl Https::Server for HttpsImpl {
    async fn domain(&self, name: capnp::text::Reader) {
        let domain_impl = DomainImpl::new(name.to_str()?, self.https_client.clone())?;
        let domain: Domain::Client = capnp_rpc::new_client(domain_impl);
        results.get().set_result(domain);
        Ok(())
    }
}
