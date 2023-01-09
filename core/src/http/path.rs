use std::str::FromStr;

use crate::http_capnp::path as Path;
use capnp::capability::Promise;
use capnp_rpc::pry;
use futures::TryFutureExt;
use hyper::{
    client::{HttpConnector, ResponseFuture},
    http::uri::{Authority, Parts},
    HeaderMap,
};
use hyper_tls::HttpsConnector;

fn get_uri(authority: &Authority) -> hyper::Uri {
    let mut parts = Parts::default();
    parts.scheme = Some("https".parse().unwrap());
    parts.authority = Some(authority.clone()); // TODO Clone?
    parts.path_and_query = Some("".parse().unwrap());
    // TODO parts.path_and_query
    hyper::Uri::from_parts(parts).unwrap() // TODO unwrap
}

pub struct PathImpl {
    // TODO Client is cheap to clone and cloning is the recommended way to share a Client. The underlying connection pool will be reused.
    // We're not doing that - should we? And if so, does it trickle down?
    https_client: hyper::Client<HttpsConnector<HttpConnector>>,
    query: Vec<(String, String)>,
    headers: HeaderMap,
    query_modifiable: bool,
    path_modifiable: bool,
    headers_modifiable: bool,
    authority: Authority,
}

impl PathImpl {
    pub fn new(path_name: String) -> Self {
        // TODO String should be something that coerces to String
        let connector = HttpsConnector::new();
        let https_client = hyper::Client::builder().build::<_, hyper::Body>(connector);
        PathImpl {
            query: vec![],
            headers: HeaderMap::new(),
            path_modifiable: true,
            query_modifiable: true,
            headers_modifiable: true,
            https_client,
            authority: Authority::from_str(path_name.as_str()).unwrap(), // TODO unwrap
        }
    }

    fn http_request_impl(
        &mut self,
        mut results: capnp::capability::Results<crate::http_capnp::path::http_result::Owned>,
        verb: Path::HttpVerb,
    ) -> Promise<(), capnp::Error> {
        let future = self.http_request(Path::HttpVerb::Get, None);
        Promise::<_, capnp::Error>::from_future(async move {
            let mut results_builder = results.get();
            let response = future
                .await
                .map_err(|err| capnp::Error::failed(err.to_string()))?;
            results_builder.set_status_code(response.status().as_u16());
            let len_response_headers: u32 = response.headers().len() as u32;
            let header_iter = response.headers().iter().enumerate();
            let mut results_headers = results_builder
                .reborrow()
                .init_headers(len_response_headers);
            for (i, (key, value)) in header_iter {
                let mut pair = results_headers.reborrow().get(i as u32);
                pair.set_key(key.as_str());
                pair.set_value(value.to_str().unwrap()); // TODO Another risky unwrap
            }
            let body_bytes = hyper::body::to_bytes(response.into_body())
                .await
                .map_err(|err| capnp::Error::failed(err.to_string()))?;
            let body = String::from_utf8(body_bytes.to_vec()).unwrap(); // TODO unwrapping not what we want
            results_builder.reborrow().set_body(body.as_str());
            Ok(())
        })
    }

    // Such function isn't part of specification, but I think it'd simplify interface
    fn http_request(&self, verb: Path::HttpVerb, body: Option<String>) -> ResponseFuture {
        let has_empty_body = [
            Path::HttpVerb::Get,
            Path::HttpVerb::Head,
            Path::HttpVerb::Options,
        ]
        .contains(&verb);
        let method = match verb {
            Path::HttpVerb::Get => "GET",
            Path::HttpVerb::Head => "HEAD",
            Path::HttpVerb::Post => "POST",
            Path::HttpVerb::Put => "PUT",
            Path::HttpVerb::Delete => "DELETE",
            Path::HttpVerb::Options => "OPTIONS",
            Path::HttpVerb::Patch => "PATCH",
        };
        let mut request_builder = hyper::Request::builder()
            .method(method)
            .uri(get_uri(&self.authority));
        for (key, value) in self.headers.iter() {
            request_builder = request_builder.header(key, value);
        }
        if has_empty_body || body.is_none() {
            let request = request_builder.body(hyper::Body::empty()).unwrap();
            self.https_client.request(request)
        } else {
            let request = request_builder.body(body.unwrap().into()).unwrap(); //TODO Unwrap here and 3 above and no verification that body isn't malicious
            self.https_client.request(request)
        }
    }
}

impl Path::Server for PathImpl {
    fn query(
        &mut self,
        params: Path::QueryParams,
        mut results: Path::QueryResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
        results.get().set_result(value);
        Promise::ok(())
    }

    fn path(
        &mut self,
        params: Path::PathParams,
        mut results: Path::PathResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
        results.get().set_result(value);
        Promise::ok(())
    }
    // START OF IMPLEMENTATIONS THAT RETURN HTTP RESULT
    fn get_http(
        &mut self,
        _: Path::GetHttpParams,
        mut results: Path::GetHttpResults,
    ) -> Promise<(), capnp::Error> {
        let future = self.http_request(Path::HttpVerb::Get, None);
        //let future = self.https_client.get(get_uri(&self.authority));
        Promise::<_, capnp::Error>::from_future(async move {
            let mut results_builder = results.get().init_result();
            let response = future
                .await
                .map_err(|err| capnp::Error::failed(err.to_string()))?;
            results_builder.set_status_code(response.status().as_u16());
            let len_response_headers: u32 = response.headers().len() as u32;
            let header_iter = response.headers().iter().enumerate();
            let mut results_headers = results_builder
                .reborrow()
                .init_headers(len_response_headers);
            for (i, (key, value)) in header_iter {
                let mut pair = results_headers.reborrow().get(i as u32);
                pair.set_key(key.as_str());
                pair.set_value(value.to_str().unwrap()); // TODO Another risky unwrap
            }
            let body_bytes = hyper::body::to_bytes(response.into_body())
                .await
                .map_err(|err| capnp::Error::failed(err.to_string()))?;
            let body = String::from_utf8(body_bytes.to_vec()).unwrap(); // TODO unwrapping not what we want
            results_builder.reborrow().set_body(body.as_str());
            Ok(())
        })
    }

    fn head(
        &mut self,
        _: Path::HeadParams,
        mut results: Path::HeadResults,
    ) -> Promise<(), capnp::Error> {
        let future = self.http_request(Path::HttpVerb::Head, None);

        // TODO The same contents as in get Promise exactly
        Promise::ok(())
    }

    fn post(
        &mut self,
        params: Path::PostParams,
        mut results: Path::PostResults,
    ) -> Promise<(), capnp::Error> {
        let body = pry!(pry!(params.get()).get_body());
        let future = self.http_request(Path::HttpVerb::Post, Some(body.to_string())); //TODO to_string should not be needed
        self.http_request_impl(results.into(), Path::HttpVerb::Post)
        // TODO The same contents as in get Promise exactly
    }

    fn put(
        &mut self,
        params: Path::PutParams,
        mut results: Path::PutResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
        results.get().set_result(value);
        Promise::ok(())
    }

    fn delete(
        &mut self,
        params: Path::DeleteParams,
        mut results: Path::DeleteResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
        results.get().set_result(value);
        Promise::ok(())
    }

    fn options(
        &mut self,
        _: Path::OptionsParams,
        mut results: Path::OptionsResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
        results.get().set_result(value);
        Promise::ok(())
    }

    fn patch(
        &mut self,
        params: Path::PatchParams,
        mut results: Path::PatchResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
        results.get().set_result(value);
        Promise::ok(())
    }
    // END OF IMPLEMENTATIONS THAT RETURN HTTP RESULT
    fn finalize_query(
        &mut self,
        _: Path::FinalizeQueryParams,
        mut results: Path::FinalizeQueryResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
        results.get().set_result(value);
        Promise::ok(())
    }

    fn finalize_path(
        &mut self,
        _: Path::FinalizePathParams,
        mut results: Path::FinalizePathResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
        results.get().set_result(value);
        Promise::ok(())
    }

    fn whitelist_verbs(
        &mut self,
        _: Path::WhitelistVerbsParams,
        mut results: Path::WhitelistVerbsResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
        results.get().set_result(value);
        Promise::ok(())
    }

    fn headers(
        &mut self,
        params: Path::HeadersParams,
        mut results: Path::HeadersResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
        results.get().set_result(value);
        Promise::ok(())
    }

    fn finalize_headers(
        &mut self,
        _: Path::FinalizeHeadersParams,
        mut results: Path::FinalizeHeadersResults,
    ) -> Promise<(), capnp::Error> {
        let value = todo!();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_test() {
        let path_client: Path::Client =
            capnp_rpc::new_client(PathImpl::new("www.example.org".to_string()));
        let request = path_client.get_http_request();
        let result = request.send().promise.await.unwrap();
        let res = result.get().unwrap().get_result().unwrap();
        let body = res.get_body().unwrap();
        let headers = res.get_headers().unwrap();
        let status = res.get_status_code();
        assert_eq!(status, 200); // 200 OK
        println!("Body: {}", body);
        //TODO display query
    }
}
