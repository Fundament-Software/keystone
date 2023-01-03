use crate::http_capnp::path as Path;
use capnp_rpc::pry;

struct PathImpl {
    query: Vec<(String, String)>,
}

impl Path::Server for PathImpl {
    fn query(
        &mut self,
        params: Path::QueryParams,
        mut results: Path::QueryResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let reader = pry!(params.get());
        let key = pry!(reader.get_key());
        let value = pry!(reader.get_value());
        self.query.push((String::from(key), String::from(value)));
        //results.get().set_path(self);
        capnp::capability::Promise::ok(())
    }

    fn path(
        &mut self,
        _: Path::PathParams,
        _: Path::PathResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        capnp::capability::Promise::err(capnp::Error::unimplemented(
            "method path::Server::path not implemented".to_string(),
        ))
    }

    fn subpath(
        &mut self,
        _: Path::SubpathParams,
        _: Path::SubpathResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        capnp::capability::Promise::err(capnp::Error::unimplemented(
            "method path::Server::subpath not implemented".to_string(),
        ))
    }

    fn get_http(
        &mut self,
        _: Path::GetHttpParams,
        _: Path::GetHttpResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        capnp::capability::Promise::err(capnp::Error::unimplemented(
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
