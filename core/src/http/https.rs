use super::domain::DomainImpl;
use super::{Domain, Https};
use capnp_macros::capnproto_rpc;
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::{Client as HttpClient, connect::HttpConnector};
use hyper_util::rt::TokioExecutor;
use std::rc::Rc;

pub struct HttpsImpl {
    https_client: HttpClient<HttpsConnector<HttpConnector>, String>,
}

impl HttpsImpl {
    pub fn new() -> Self {
        let connector = HttpsConnector::new();
        let https_client = HttpClient::builder(TokioExecutor::new()).build::<_, String>(connector);
        HttpsImpl { https_client }
    }
}
#[capnproto_rpc(Https)]
impl Https::Server for HttpsImpl {
    async fn domain(self: Rc<Self>, name: capnp::text::Reader) {
        let domain_impl = DomainImpl::new(name.to_str()?, self.https_client.clone())?;
        let domain: Domain::Client = capnp_rpc::new_client(domain_impl);
        results.get().set_result(domain);
        Ok(())
    }
}
