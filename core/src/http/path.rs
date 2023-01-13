use super::Path;
use capnp::capability::Promise;
use capnp_rpc::pry;
use hyper::{
    client::{HttpConnector, ResponseFuture},
    http::uri::{Authority, Parts, PathAndQuery},
    HeaderMap,
};
use hyper_tls::HttpsConnector;
use Path::HttpVerb;

macro_rules! http_request_promise {
    ($results:expr, $future:expr) => {
        Promise::<_, capnp::Error>::from_future(async move {
            let mut results_builder = $results.get().init_result();
            let response = $future
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
                pair.set_value(value.to_str().map_err(|_| {
                    capnp::Error::failed("Response Header contains invalid character".to_string())
                })?);
            }
            let body_bytes = hyper::body::to_bytes(response.into_body())
                .await
                .map_err(|err| capnp::Error::failed(err.to_string()))?;
            let body = String::from_utf8(body_bytes.to_vec()).map_err(|_| {
                capnp::Error::failed("Couldn't convert response body to utf8 String".to_string())
            })?;
            results_builder.reborrow().set_body(body.as_str());
            Ok(())
        })
    };
}

#[derive(Clone)]
pub struct PathImpl {
    https_client: hyper::Client<HttpsConnector<HttpConnector>>,
    query: Vec<(String, String)>,
    path_list: Vec<String>,
    headers: HeaderMap,
    query_modifiable: bool,
    path_modifiable: bool,
    headers_modifiable: bool,
    authority: Authority,
    verb_whitelist: Vec<HttpVerb>, //TODO consider a set instead
}

impl PathImpl {
    pub fn new<A: TryInto<Authority>, S: Into<String>>(
        authority: A,
        initial_path: S,
    ) -> Result<Self, capnp::Error> {
        let connector = HttpsConnector::new();
        // TODO Client is cheap to clone and cloning is the recommended way to share a Client. The underlying connection pool will be reused.
        // We're not doing that - should we? And if so, does it trickle down?
        let https_client = hyper::Client::builder().build::<_, hyper::Body>(connector);
        let path_item: String = initial_path.into();
        let mut path_list = vec![];
        if !path_item.is_empty() {
            path_list.push(path_item);
        }
        Ok(PathImpl {
            https_client,
            query: vec![],
            path_list,
            headers: HeaderMap::new(),
            path_modifiable: true,
            query_modifiable: true,
            headers_modifiable: true,
            verb_whitelist: vec![],
            authority: authority.try_into().map_err(|_| {
                capnp::Error::failed("Couldn't create path - invalid authority".to_string())
            })?,
        })
    }

    fn get_uri(&self) -> capnp::Result<hyper::Uri> {
        // TODO Use something well-established, like https://docs.rs/url/latest/url/ - apply to other places
        let mut parts = Parts::default();
        parts.scheme = Some("https".parse().unwrap()); // Ok to unwrap - will always parse. TODO - should we allow http too?
        parts.authority = Some(self.authority.clone()); // TODO Clone?
        parts.path_and_query = Some(self.path_and_query()?);
        hyper::Uri::from_parts(parts).map_err(|_| capnp::Error::failed("Invalid URL".to_string()))
    }

    fn path_and_query(&self) -> capnp::Result<PathAndQuery> {
        let mut path = String::from("");
        for i in self.path_list.iter() {
            path.push('/');
            path.push_str(i);
        }
        let mut query = String::from("");
        if !self.query.is_empty() {
            query.push('?');
        }
        for (k, v) in self.query.iter() {
            if !query.ends_with('?') {
                query.push('&');
            }
            query.push_str(format!("{k}={v}").as_str());
        }
        PathAndQuery::try_from(path + query.as_str())
            .map_err(|_| capnp::Error::failed("Couldn't construct PathAndQuery".to_string()))
    }

    // Such function isn't part of specification, but I think it'd simplify interface
    fn http_request(
        &self,
        verb: HttpVerb,
        body: Option<String>,
    ) -> Result<ResponseFuture, capnp::Error> {
        let method = match verb {
            HttpVerb::Get => "GET",
            HttpVerb::Head => "HEAD",
            HttpVerb::Post => "POST",
            HttpVerb::Put => "PUT",
            HttpVerb::Delete => "DELETE",
            HttpVerb::Options => "OPTIONS",
            HttpVerb::Patch => "PATCH",
        };
        if !self.verb_whitelist.is_empty() && self.verb_whitelist.contains(&verb) {
            return Err(capnp::Error::failed(
                "{method} is not on a whitelist and can't be executed.".to_string(),
            ));
        }
        let has_empty_body = [HttpVerb::Get, HttpVerb::Head, HttpVerb::Options].contains(&verb);

        let mut request_builder = hyper::Request::builder()
            .method(method)
            .uri(self.get_uri()?);
        for (key, value) in self.headers.iter() {
            request_builder = request_builder.header(key, value);
        }

        let request_body: hyper::Body = match body {
            Some(_) if has_empty_body => hyper::Body::empty(),
            Some(body) => body.into(),
            None => hyper::Body::empty(),
        };

        let request = request_builder
            .body(request_body)
            .map_err(|_| capnp::Error::failed("Couldn't construct https request".to_string()))?;
        Ok(self.https_client.request(request))
    }
}

impl Path::Server for PathImpl {
    fn query(
        &mut self,
        params: Path::QueryParams,
        mut results: Path::QueryResults,
    ) -> Promise<(), capnp::Error> {
        if !self.query_modifiable {
            return Promise::err(capnp::Error::failed(
                "can't add to query, because it was finalized".to_string(),
            ));
        }
        let values = pry!(pry!(params.get()).get_values());
        let mut return_path = self.clone();
        for value in values.iter() {
            let k = pry!(value.get_key());
            let v = pry!(value.get_value());
            return_path.query.push((k.to_string(), v.to_string()));
        }
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Promise::ok(())
    }

    fn path(
        &mut self,
        params: Path::PathParams,
        mut results: Path::PathResults,
    ) -> Promise<(), capnp::Error> {
        if !self.path_modifiable {
            return Promise::err(capnp::Error::failed(format!(
                "can't add to path, because it was finalized"
            )));
        }
        let values = pry!(pry!(params.get()).get_values());
        let mut return_path = self.clone();
        for value in values.iter() {
            let v = pry!(value);
            return_path.path_list.push(v.to_string());
        }
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Promise::ok(())
    }
    // START OF IMPLEMENTATIONS THAT RETURN HTTP RESULT
    fn get(
        &mut self,
        _: Path::GetParams,
        mut results: Path::GetResults,
    ) -> Promise<(), capnp::Error> {
        let future = self.http_request(HttpVerb::Get, None);
        match future {
            Ok(f) => http_request_promise!(results, f),
            Err(e) => Promise::err(e),
        }
    }

    fn head(
        &mut self,
        _: Path::HeadParams,
        mut results: Path::HeadResults,
    ) -> Promise<(), capnp::Error> {
        let future = self.http_request(HttpVerb::Head, None);
        match future {
            Ok(f) => http_request_promise!(results, f),
            Err(e) => Promise::err(e),
        }
    }

    fn post(
        &mut self,
        params: Path::PostParams,
        mut results: Path::PostResults,
    ) -> Promise<(), capnp::Error> {
        let body = pry!(pry!(params.get()).get_body());
        let future = self.http_request(HttpVerb::Post, Some(body.to_string())); //TODO to_string should not be needed in all those methods
        match future {
            Ok(f) => http_request_promise!(results, f),
            Err(e) => Promise::err(e),
        }
    }

    fn put(
        &mut self,
        params: Path::PutParams,
        mut results: Path::PutResults,
    ) -> Promise<(), capnp::Error> {
        let body = pry!(pry!(params.get()).get_body());
        let future = self.http_request(HttpVerb::Put, Some(body.to_string()));
        match future {
            Ok(f) => http_request_promise!(results, f),
            Err(e) => Promise::err(e),
        }
    }

    fn delete(
        &mut self,
        params: Path::DeleteParams,
        mut results: Path::DeleteResults,
    ) -> Promise<(), capnp::Error> {
        let body = pry!(pry!(params.get()).get_body());
        let future = self.http_request(HttpVerb::Delete, Some(body.to_string()));
        match future {
            Ok(f) => http_request_promise!(results, f),
            Err(e) => Promise::err(e),
        }
    }

    fn options(
        &mut self,
        _: Path::OptionsParams,
        mut results: Path::OptionsResults,
    ) -> Promise<(), capnp::Error> {
        let future = self.http_request(HttpVerb::Options, None);
        match future {
            Ok(f) => http_request_promise!(results, f),
            Err(e) => Promise::err(e),
        }
    }

    fn patch(
        &mut self,
        params: Path::PatchParams,
        mut results: Path::PatchResults,
    ) -> Promise<(), capnp::Error> {
        let body = pry!(pry!(params.get()).get_body());
        let future = self.http_request(HttpVerb::Patch, Some(body.to_string()));
        match future {
            Ok(f) => http_request_promise!(results, f),
            Err(e) => Promise::err(e),
        }
    }
    // END OF IMPLEMENTATIONS THAT RETURN HTTP RESULT
    fn finalize_query(
        &mut self,
        _: Path::FinalizeQueryParams,
        mut results: Path::FinalizeQueryResults,
    ) -> Promise<(), capnp::Error> {
        let mut return_path = self.clone();
        return_path.query_modifiable = false;
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Promise::ok(())
    }

    fn finalize_path(
        &mut self,
        _: Path::FinalizePathParams,
        mut results: Path::FinalizePathResults,
    ) -> Promise<(), capnp::Error> {
        let mut return_path = self.clone();
        return_path.path_modifiable = false;
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Promise::ok(())
    }

    fn whitelist_verbs(
        &mut self,
        params: Path::WhitelistVerbsParams,
        mut results: Path::WhitelistVerbsResults,
    ) -> Promise<(), capnp::Error> {
        // TODO No pry!
        let verbs = pry!(pry!(params.get()).get_verbs());
        let mut return_path = self.clone();
        for verb in verbs.iter() {
            let v = pry!(verb);
            // Check uniqueness - would use HashSet, but enums aren't Hash by default.
            // TODO Make sure behavior we want is extending whitelist, not overriding it
            if !return_path.verb_whitelist.contains(&v) {
                return_path.verb_whitelist.push(v);
            }
        }
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Promise::ok(())
    }

    fn headers(
        &mut self,
        params: Path::HeadersParams,
        mut results: Path::HeadersResults,
    ) -> Promise<(), capnp::Error> {
        if !self.headers_modifiable {
            return Promise::err(capnp::Error::failed(
                "Can't add headers, because they were finalized".to_string(),
            ));
        }
        let headers = pry!(pry!(params.get()).get_headers());
        let mut return_path = self.clone();
        for header in headers.iter() {
            let key = pry!(header.get_key());
            let key = hyper::header::HeaderName::try_from(key);
            if let Err(_) = key {
                return Promise::err(capnp::Error::failed(
                    "Can't add header, key is invalid".to_string(),
                ));
            }
            let key = key.unwrap();
            let value = pry!(header.get_value());
            let value = hyper::http::HeaderValue::try_from(value);
            if let Err(_) = value {
                return Promise::err(capnp::Error::failed(
                    "Can't add header, value is invalid".to_string(),
                ));
            }
            let value = value.unwrap();
            return_path.headers.append(key, value);
        }
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Promise::ok(())
    }

    fn finalize_headers(
        &mut self,
        _: Path::FinalizeHeadersParams,
        mut results: Path::FinalizeHeadersResults,
    ) -> Promise<(), capnp::Error> {
        let mut return_path = self.clone();
        return_path.headers_modifiable = false;
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Promise::ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_test() -> capnp::Result<()> {
        // Current way to run it and see results: cargo test -- --nocapture
        let mut path_client: Path::Client =
            capnp_rpc::new_client(PathImpl::new("httpbin.org", "")?);

        let mut request = path_client.path_request();
        {
            let mut path_params = request.get().init_values(1); //one element
            path_params.set(0, "get");
        }
        path_client = request.send().promise.await?.get()?.get_result()?;
        let mut request = path_client.query_request();
        {
            let mut query_params = request.get().init_values(3);
            query_params.reborrow().get(0).set_key("key1");
            query_params.reborrow().get(0).set_value("value1");
            query_params.reborrow().get(1).set_key("key2");
            query_params.reborrow().get(1).set_value("value2");
            query_params.reborrow().get(2).set_key("key3");
            query_params.reborrow().get(2).set_value("value3");
        }
        path_client = request.send().promise.await?.get()?.get_result()?;
        let request = path_client.get_request();
        let result = request.send().promise.await?;
        let result = result.get()?.get_result()?;

        let body = result.get_body()?;
        let response_headers = result.get_headers()?;
        println!("Headers:");
        for response_header in response_headers.iter() {
            let key = response_header.get_key()?;
            let value = response_header.get_value()?;
            println!("\tKey: {key}\n\tValue: {value}\n----------------")
        }
        let status = result.get_status_code();
        assert_eq!(status, 200); // 200 OK
        println!("Body: {}", body);
        Ok(())
    }

    #[tokio::test]
    async fn post_test() -> capnp::Result<()> {
        // Current way to run it and see results: cargo test -- --nocapture
        let mut path_client: Path::Client =
            capnp_rpc::new_client(PathImpl::new("httpbin.org", "post")?);

        let mut request = path_client.query_request();
        {
            let mut query_params = request.get().init_values(3);
            query_params.reborrow().get(0).set_key("key1");
            query_params.reborrow().get(0).set_value("value1");
            query_params.reborrow().get(1).set_key("key2");
            query_params.reborrow().get(1).set_value("value2");
            query_params.reborrow().get(2).set_key("key3");
            query_params.reborrow().get(2).set_value("value3");
        }
        path_client = request.send().promise.await?.get()?.get_result()?;
        let mut request = path_client.post_request();
        {
            request
                .get()
                .set_body("Here's something I post. It should be returned to me");
        }
        let result = request.send().promise.await?;
        let result = result.get()?.get_result()?;

        let body = result.get_body()?;
        let response_headers = result.get_headers()?;
        println!("Headers:");
        for response_header in response_headers.iter() {
            let key = response_header.get_key()?;
            let value = response_header.get_value()?;
            println!("\tKey: {key}\n\tValue: {value}\n----------------")
        }
        let status = result.get_status_code();
        assert_eq!(status, 200); // 200 OK
        println!("Body: {}", body);
        Ok(())
    }
}
