use super::path::PathImpl;
use super::{Domain, Path};
use capnp::capability::Promise;
use capnp_rpc::pry;
use hyper::client::HttpConnector;
use hyper::http::uri::Authority;
use hyper_tls::HttpsConnector;

#[derive(Clone)]
pub struct DomainImpl {
    domain_name: Authority,
    https_client: hyper::Client<HttpsConnector<HttpConnector>>,
    modifiable: bool,
}

impl DomainImpl {
    pub fn new<A: TryInto<Authority>>(
        domain_name: A,
        https_client: hyper::Client<HttpsConnector<HttpConnector>>,
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

impl Domain::Server for DomainImpl {
    fn subdomain(
        &mut self,
        params: Domain::SubdomainParams,
        mut results: Domain::SubdomainResults,
    ) -> Promise<(), capnp::Error> {
        if !self.modifiable {
            return Promise::err(capnp::Error::failed(
                "Can't add subdomain, because domain was finalized".to_string(),
            ));
        }
        let original_domain_name = self.domain_name.clone();
        let name = pry!(pry!(params.get()).get_name());
        let new_domain_name = name.to_string() + "." + original_domain_name.as_str();
        let domain_impl = DomainImpl::new(new_domain_name, self.https_client.clone());
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
        let path_list: Vec<String> = pry!(pry!(params.get()).get_values())
            .iter()
            .filter(|i| i.is_ok()) // TODO Not sure what the errors would be, we should probably report them instead of skipping
            .map(|i| i.unwrap().to_string())
            .collect();
        let path_impl = PathImpl::new(
            self.domain_name.as_str(),
            path_list,
            self.https_client.clone(),
        );
        if let Err(e) = path_impl {
            return Promise::err(e);
        }
        let path: Path::Client = capnp_rpc::new_client(path_impl.unwrap());
        results.get().set_result(path);
        Promise::ok(())
    }

    fn finalize_domain(
        &mut self,
        _: Domain::FinalizeDomainParams,
        mut results: Domain::FinalizeDomainResults,
    ) -> Promise<(), capnp::Error> {
        let mut return_domain = self.clone();
        return_domain.modifiable = false;
        let client = capnp_rpc::new_client(return_domain);
        results.get().set_result(client);
        Promise::ok(())
    }
}
