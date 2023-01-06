use crate::http_capnp::domain as Domain;
use crate::http_capnp::https as Https;
use crate::http_capnp::path as Path;
use capnp::capability::Promise;
use capnp_rpc::pry;

pub struct PathImpl {
    query: Vec<(String, String)>,
}

impl PathImpl {
    pub fn new(path_name: String, https_cap: Https::Client, domain_cap: Domain::Client) -> Self {
        // TODO String should be something that coerces to String
        return PathImpl { query: todo!() };
    }
}

impl Path::Server for PathImpl {
    fn query(
        &mut self,
        params: Path::QueryParams,
        mut results: Path::QueryResults,
    ) -> Promise<(), capnp::Error> {
        let reader = pry!(params.get());
        let key = pry!(reader.get_key());
        let value = pry!(reader.get_value());
        self.query.push((String::from(key), String::from(value)));
        //results.get().set_path(self);
        Promise::ok(())
    }

    fn path(&mut self, _: Path::PathParams, _: Path::PathResults) -> Promise<(), capnp::Error> {
        Promise::err(capnp::Error::unimplemented(
            "method path::Server::path not implemented".to_string(),
        ))
    }

    fn subpath(
        &mut self,
        _: Path::SubpathParams,
        _: Path::SubpathResults,
    ) -> Promise<(), capnp::Error> {
        Promise::err(capnp::Error::unimplemented(
            "method path::Server::subpath not implemented".to_string(),
        ))
    }

    fn get_http(
        &mut self,
        _: Path::GetHttpParams,
        _: Path::GetHttpResults,
    ) -> Promise<(), capnp::Error> {
        Promise::err(capnp::Error::unimplemented(
            "method path::Server::get not implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_test() {
        let path_client: Path::Client = capnp_rpc::new_client(PathImpl { query: vec![] });
        let mut request = path_client.query_request();
        request.get().set_key("3");
        request.get().set_value("apple");
        let path = request.send().pipeline.get_path();
        //TODO display query
    }
}
