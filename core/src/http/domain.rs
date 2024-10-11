use super::path::PathImpl;
use super::{Domain, Path};
use capnp_macros::capnproto_rpc;
use hyper::http::uri::Authority;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::{connect::HttpConnector, Client as HttpClient};

#[derive(Clone)]
pub struct DomainImpl {
    domain_name: Authority,
    https_client: HttpClient<HttpsConnector<HttpConnector>, String>,
    modifiable: bool,
}

impl DomainImpl {
    pub fn new<A: TryInto<Authority>>(
        domain_name: A,
        https_client: HttpClient<HttpsConnector<HttpConnector>, String>,
    ) -> Result<Self, capnp::Error> {
        Ok(DomainImpl {
            https_client,
            domain_name: domain_name.try_into().map_err(|_| {
                capnp::Error::failed("Can't create domain - invalid authority".to_string())
            })?,
            modifiable: true,
        })
    }
}
#[capnproto_rpc(Domain)]
impl Domain::Server for DomainImpl {
    async fn subdomain(&self, name: capnp::text::Reader) {
        if !self.modifiable {
            return Err(capnp::Error::failed(
                "Can't add subdomain, because domain was finalized".to_string(),
            ));
        }
        let original_domain_name = self.domain_name.clone();
        let new_domain_name = name.to_string()? + "." + original_domain_name.as_str();
        let domain_impl = DomainImpl::new(new_domain_name, self.https_client.clone())?;
        let domain: Domain::Client = capnp_rpc::new_client(domain_impl);
        results.get().set_result(domain);
        Ok(())
    }

    async fn path(&self, values: capnp::text_list::Reader) {
        let path_list: Result<Vec<String>, capnp::Error> =
            values.iter().map(|i| Ok(i?.to_string()?)).collect();
        let path_impl = PathImpl::new(
            self.domain_name.as_str(),
            path_list?,
            self.https_client.clone(),
        )?;
        let path: Path::Client = capnp_rpc::new_client(path_impl);
        results.get().set_result(path);
        Ok(())
    }

    async fn finalize_domain(&self) {
        let mut return_domain = self.clone();
        return_domain.modifiable = false;
        let client = capnp_rpc::new_client(return_domain);
        results.get().set_result(client);
        Ok(())
    }
}
