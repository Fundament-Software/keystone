use crate::http_capnp::domain as Domain;
use crate::http_capnp::https as Https;
use crate::http_capnp::path as Path;
use capnp::capability::Promise;
use capnp_rpc::pry;

pub struct PathImpl {
    // TODO Technically https_cap can be obtained indirectly from domain_cap - should it? Also, there is capability_list, so they can be combined?
    https_cap: Https::Client,
    domain_cap: Domain::Client,
    query_modifiable: bool,
    path_modifiable: bool,
    headers_modifiable: bool,
    query: Vec<(String, String)>,
}

impl PathImpl {
    pub fn new(path_name: String, https_cap: Https::Client, domain_cap: Domain::Client) -> Self {
        // TODO String should be something that coerces to String
        return PathImpl {
            query: vec![],
            https_cap,
            domain_cap,
            path_modifiable: true,
            query_modifiable: true,
            headers_modifiable: true,
        };
    }

    // Such function isn't part of specification, but I think it'd simplify interface
    fn http_request(&mut self, verb: Path::HttpVerb, body: Option<String>) {
        match verb {
            Path::HttpVerb::Get => todo!(),
            Path::HttpVerb::Head => todo!(),
            Path::HttpVerb::Post => todo!(),
            Path::HttpVerb::Put => todo!(),
            Path::HttpVerb::Delete => todo!(),
            Path::HttpVerb::Options => todo!(),
            Path::HttpVerb::Patch => todo!(),
        }
    }
}

#[rustfmt::skip]
impl Path::Server for PathImpl {
    fn query(&mut self, params: Path::QueryParams<>, mut results: Path::QueryResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}

    fn path(&mut self, params: Path::PathParams<>, mut results: Path::PathResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}
    // START OF IMPLEMENTATIONS THAT RETURN HTTP RESULT
    fn get_http(&mut self,_:Path::GetHttpParams<>, mut results: Path::GetHttpResults<>) ->  Promise<(), capnp::Error>{
        // CURRENT TODO - figure out how to get underlying hyper::Client object
        self.https_cap.;
        results.get().set_result(value);
        Promise::ok(())
}

    fn head(&mut self,_:Path::HeadParams<>, mut results: Path::HeadResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}

    fn post(&mut self, params: Path::PostParams<>, mut results: Path::PostResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}

    fn put(&mut self, params: Path::PutParams<>, mut results: Path::PutResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}

    fn delete(&mut self, params: Path::DeleteParams<>, mut results: Path::DeleteResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}

    fn options(&mut self,_:Path::OptionsParams<>, mut results: Path::OptionsResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}

    fn patch(&mut self, params: Path::PatchParams<>, mut results: Path::PatchResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}
    // END OF IMPLEMENTATIONS THAT RETURN HTTP RESULT
    fn finalize_query(&mut self,_:Path::FinalizeQueryParams<>, mut results: Path::FinalizeQueryResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}

    fn finalize_path(&mut self,_:Path::FinalizePathParams<>, mut results: Path::FinalizePathResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}

    fn whitelist_verbs(&mut self,_:Path::WhitelistVerbsParams<>, mut results: Path::WhitelistVerbsResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}

    fn headers(&mut self, params: Path::HeadersParams<>, mut results: Path::HeadersResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())
}

    fn finalize_headers(&mut self,_:Path::FinalizeHeadersParams<>, mut results: Path::FinalizeHeadersResults<>) ->  Promise<(), capnp::Error>{
        results.get().set_result(value);
        Promise::ok(())

}
}

// impl Path::Server for PathImpl {
//     fn query(
//         &mut self,
//         params: Path::QueryParams,
//         mut results: Path::QueryResults,
//     ) -> Promise<(), capnp::Error> {
//         let reader = pry!(params.get());
//         let key = pry!(reader.get_key());
//         let value = pry!(reader.get_value());
//         self.query.push((String::from(key), String::from(value)));
//         //results.get().set_path(self);
//         Promise::ok(())
//     }

//     fn path(&mut self, _: Path::PathParams, _: Path::PathResults) -> Promise<(), capnp::Error> {
//         Promise::err(capnp::Error::unimplemented(
//             "method path::Server::path not implemented".to_string(),
//         ))
//     }

//     fn subpath(
//         &mut self,
//         _: Path::SubpathParams,
//         _: Path::SubpathResults,
//     ) -> Promise<(), capnp::Error> {
//         Promise::err(capnp::Error::unimplemented(
//             "method path::Server::subpath not implemented".to_string(),
//         ))
//     }

//     fn get_http(
//         &mut self,
//         _: Path::GetHttpParams,
//         _: Path::GetHttpResults,
//     ) -> Promise<(), capnp::Error> {
//         Promise::err(capnp::Error::unimplemented(
//             "method path::Server::get not implemented".to_string(),
//         ))
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_test() {
        let path_client: Path::Client = capnp_rpc::new_client(PathImpl { query: vec![] });
        let mut request = path_client.query_request();
        request.get().set_values("apple");
        let path = request.send().pipeline.get_path();
        //TODO display query
    }
}
