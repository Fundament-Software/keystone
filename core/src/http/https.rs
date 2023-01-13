use super::domain::DomainImpl;
use super::{Domain, Https};
use capnp::capability::Promise;
use capnp_rpc::pry;

pub struct HttpsImpl;

impl Https::Server for HttpsImpl {
    fn domain(
        &mut self,
        params: Https::DomainParams,
        mut results: Https::DomainResults,
    ) -> Promise<(), capnp::Error> {
        let domain_name = pry!(pry!(params.get()).get_name());
        let domain: Domain::Client = capnp_rpc::new_client(DomainImpl::new(domain_name));
        results.get().set_result(domain);
        Promise::ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session() {
        let url = "https://www.example.com";
        let https: Https::Client = capnp_rpc::new_client(HttpsImpl);
        let mut request = https.domain_request();
        request.get().set_name(url);
        let domain = request.send().pipeline.get_result();
        let mut request = domain.path_request();
        request.get().set_name("")
    }
}
