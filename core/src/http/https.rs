use super::domain::DomainImpl;
use super::{Domain, Https};
use capnp_macros::capnproto_rpc;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::{connect::HttpConnector, Client as HttpClient};
use hyper_util::rt::TokioExecutor;

pub struct HttpsImpl {
    https_client: HttpClient<HttpsConnector<HttpConnector>, String>,
}

impl HttpsImpl {
    pub fn new() -> Self {
        let connector = HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_only()
            .enable_all_versions()
            .build();
        let https_client = HttpClient::builder(TokioExecutor::new()).build::<_, String>(connector);
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
