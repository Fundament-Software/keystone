use super::Path;
use capnp_macros::capnproto_rpc;
use http_body_util::BodyExt;
use hyper::{
    http::uri::Authority,
    HeaderMap,
};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::{
    connect::HttpConnector,
    Client as HttpClient,
    ResponseFuture
};

use Path::HttpVerb;

impl std::fmt::Display for Path::HttpVerb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let verb = match self {
            HttpVerb::Get => "GET",
            HttpVerb::Head => "HEAD",
            HttpVerb::Post => "POST",
            HttpVerb::Put => "PUT",
            HttpVerb::Delete => "DELETE",
            HttpVerb::Options => "OPTIONS",
            HttpVerb::Patch => "PATCH",
        };
        write!(f, "{}", verb)
    }
}

#[derive(Clone)]
pub struct PathImpl {
    https_client: HttpClient<HttpsConnector<HttpConnector>, String>,
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
    pub fn new<A: TryInto<Authority>, S: Into<Vec<String>>>(
        authority: A,
        path_list: S,
        https_client: HttpClient<HttpsConnector<HttpConnector>, String>,
    ) -> Result<Self, capnp::Error> {
        let verb_whitelist = vec![
            HttpVerb::Get,
            HttpVerb::Head,
            HttpVerb::Post,
            HttpVerb::Put,
            HttpVerb::Delete,
            HttpVerb::Options,
            HttpVerb::Patch,
        ];
        Ok(PathImpl {
            https_client,
            query: vec![],
            path_list: path_list.into(),
            headers: HeaderMap::new(),
            path_modifiable: true,
            query_modifiable: true,
            headers_modifiable: true,
            verb_whitelist,
            authority: authority.try_into().map_err(|_| {
                capnp::Error::failed("Couldn't create path - invalid authority".to_string())
            })?,
        })
    }

    fn get_uri(&self) -> capnp::Result<String> {
        let authority = self.authority.to_string();
        let mut url = format!("https://{}", authority)
            .parse::<url::Url>()
            .map_err(|_| capnp::Error::failed("Couldn't create base url".to_string()))?;
        url.path_segments_mut()
            .map_err(|_| capnp::Error::failed("Couldn't add path segment to url".to_string()))?
            .extend(&self.path_list);
        url.query_pairs_mut().extend_pairs(&self.query);
        Ok(url.to_string())
    }

    // Function isn't part of specification, but I think it simplifies things
    fn http_request(
        &self,
        verb: HttpVerb,
        body: Option<String>,
    ) -> Result<ResponseFuture, capnp::Error> {
        let method = verb.to_string();
        if !self.verb_whitelist.contains(&verb) {
            return Err(capnp::Error::failed(format!(
                "{method} is not on a whitelist and can't be executed"
            )));
        }
        let has_empty_body = matches!(verb, HttpVerb::Get | HttpVerb::Head | HttpVerb::Options);

        let mut request_builder = hyper::Request::builder()
            .method(method.as_str())
            .uri(self.get_uri()?);
        for (key, value) in self.headers.iter() {
            request_builder = request_builder.header(key, value);
        }

        let request_body: String = match body {
            Some(_) if has_empty_body => String::new(),
            Some(body) => body.into(),
            None => String::new(),
        };

        let request = request_builder
            .body(request_body)
            .map_err(|_| capnp::Error::failed("Couldn't construct https request".to_string()))?;
        Ok(self.https_client.request(request))
    }
}

async fn http_request_promise(
    mut results_builder: Path::http_result::Builder<'_>,
    future: ResponseFuture,
) -> core::result::Result<(), capnp::Error> {
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
        pair.set_key(key.as_str().into());
        pair.set_value(
            value
                .to_str()
                .map_err(|_| {
                    capnp::Error::failed("Response Header contains invalid character".to_string())
                })?
                .into(),
        );
    }
    let body_bytes: bytes::Bytes = response.into_body().collect()
        .await
        .map_err(|err| capnp::Error::failed(err.to_string()))?
        .to_bytes();
    let body = String::from_utf8(body_bytes.to_vec()).map_err(|_| {
        capnp::Error::failed("Couldn't convert response body to utf8 String".to_string())
    })?;
    results_builder.reborrow().set_body(body.as_str().into());
    Ok(())
}
#[capnproto_rpc(Path)]
impl Path::Server for PathImpl {
    async fn query(&self, values: Reader) {
        if !self.query_modifiable {
            return Err(capnp::Error::failed(
                "Can't add to query, because it was finalized".to_string(),
            ));
        }
        let mut return_path = self.clone();
        for value in values.iter() {
            let k = value.get_key()?.to_string()?;
            let v = value.get_value()?.to_string()?;
            return_path.query.push((k, v));
        }
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Ok(())
    }

    async fn path(&self, values: Reader) {
        if !self.path_modifiable {
            return Err(capnp::Error::failed(
                "Can't add to path, because it was finalized".to_string(),
            ));
        }
        let mut return_path = self.clone();
        for value in values.iter() {
            let v = value?.to_string()?;
            return_path.path_list.push(v);
        }
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Ok(())
    }
    // START OF IMPLEMENTATIONS THAT RETURN HTTP RESULT
    async fn get(&self) {
        let future = self.http_request(HttpVerb::Get, None)?;
        http_request_promise(results.get().init_result(), future).await
    }

    async fn head(&self) {
        let future = self.http_request(HttpVerb::Head, None)?;
        http_request_promise(results.get().init_result(), future).await
    }

    async fn post(&self, body: Reader) {
        let body = body.to_string()?;
        let future = self.http_request(HttpVerb::Post, Some(body))?;

        http_request_promise(results.get().init_result(), future).await
    }

    async fn put(&self, body: Reader) {
        let body = body.to_string()?;
        let future = self.http_request(HttpVerb::Put, Some(body))?;

        http_request_promise(results.get().init_result(), future).await
    }

    async fn delete(&self, body: Reader) {
        let body = body.to_string()?;
        let future = self.http_request(HttpVerb::Delete, Some(body))?;

        http_request_promise(results.get().init_result(), future).await
    }

    async fn options(&self) {
        let future = self.http_request(HttpVerb::Options, None)?;
        http_request_promise(results.get().init_result(), future).await
    }

    async fn patch(&self, body: Reader) {
        let body = body.to_string()?;
        let future = self.http_request(HttpVerb::Patch, Some(body))?;
        http_request_promise(results.get().init_result(), future).await
    }
    // END OF IMPLEMENTATIONS THAT RETURN HTTP RESULT
    async fn finalize_query(&self) {
        let mut return_path = self.clone();
        return_path.query_modifiable = false;
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Ok(())
    }

    async fn finalize_path(&self) {
        let mut return_path = self.clone();
        return_path.path_modifiable = false;
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Ok(())
    }

    async fn whitelist_verbs(&self, verbs: Reader) {
        let mut verbs_vec = vec![];
        for verb in verbs.iter() {
            let v = verb?;
            verbs_vec.push(v);
        }
        let mut return_path = self.clone();
        return_path
            .verb_whitelist
            .retain(|&x| verbs_vec.contains(&x));
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Ok(())
    }

    async fn headers(&self, headers: Reader) {
        if !self.headers_modifiable {
            return Err(capnp::Error::failed(
                "Can't add headers, because they were finalized".to_string(),
            ));
        }
        let mut return_path = self.clone();
        for header in headers.iter() {
            let key = header.get_key()?.to_string()?;
            let key = hyper::header::HeaderName::try_from(key);
            if key.is_err() {
                return Err(capnp::Error::failed(
                    "Can't add header, key is invalid".to_string(),
                ));
            }
            let key = key.unwrap();
            let value = header.get_value()?.to_string()?;
            let value = hyper::http::HeaderValue::try_from(value);
            if value.is_err() {
                return Err(capnp::Error::failed(
                    "Can't add header, value is invalid".to_string(),
                ));
            }
            let value = value.unwrap();
            return_path.headers.append(key, value);
        }
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Ok(())
    }

    async fn finalize_headers(&self) {
        let mut return_path = self.clone();
        return_path.headers_modifiable = false;
        let client = capnp_rpc::new_client(return_path);
        results.get().set_result(client);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_util::rt::TokioExecutor;

    #[tokio::test]
    async fn get_test() -> capnp::Result<()> {
        let connector = HttpsConnector::new();
        let https_client = HttpClient::builder(TokioExecutor::new()).build::<_, String>(connector);
        let mut path_client: Path::Client =
            capnp_rpc::new_client(PathImpl::new("httpbin.org", vec!["".into()], https_client)?);

        let mut request = path_client.path_request();
        {
            let mut path_params = request.get().init_values(1); //one element
            path_params.set(0, "get".into());
        }
        path_client = request.send().promise.await?.get()?.get_result()?;

        let request = path_client.get_request();
        let result = request.send().promise.await?;
        let result = result.get()?.get_result()?;

        let body = result.get_body()?.to_str()?;

        let body_json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(body_json["headers"]["Host"], "httpbin.org");
        assert_eq!(body_json["url"], "https://httpbin.org/get");

        let status = result.get_status_code();
        assert_eq!(status, 200); // 200 OK
        Ok(())
    }

    #[tokio::test]
    async fn whitelist_test() -> capnp::Result<()> {
        // Set url as example.org
        let connector = HttpsConnector::new();
        let https_client = HttpClient::builder(TokioExecutor::new()).build::<_, String>(connector);
        let mut path_client: Path::Client =
            capnp_rpc::new_client(PathImpl::new("example.org", vec!["".into()], https_client)?);

        // Allow only DELETE request
        let mut request = path_client.whitelist_verbs_request();
        {
            let mut whitelist = request.get().init_verbs(1);
            whitelist.set(0, HttpVerb::Delete);
        }
        path_client = request.send().promise.await?.get()?.get_result()?;

        // Doesn't actually add GET now.
        let mut request = path_client.whitelist_verbs_request();
        {
            let mut whitelist = request.get().init_verbs(2);
            whitelist.set(0, HttpVerb::Delete);
            whitelist.set(1, HttpVerb::Get);
        }
        path_client = request.send().promise.await?.get()?.get_result()?;

        // Test that GET request fails
        let request = path_client.get_request();
        assert_eq!(
            request.send().promise.await.err().map(|e| e.extra),
            Some("GET is not on a whitelist and can't be executed".to_string())
        );

        // DELETE request still works
        let request = path_client.delete_request();
        assert!(request.send().promise.await.is_ok());

        Ok(())
    }

    #[tokio::test]
    async fn post_test() -> capnp::Result<()> {
        let connector = HttpsConnector::new();
        let https_client = HttpClient::builder(TokioExecutor::new()).build::<_, String>(connector);
        let mut path_client: Path::Client = capnp_rpc::new_client(PathImpl::new(
            "httpbin.org",
            vec!["post".into()],
            https_client,
        )?);

        let mut request = path_client.query_request();
        {
            let mut query_params = request.get().init_values(3);
            query_params.reborrow().get(0).set_key("key1".into());
            query_params.reborrow().get(0).set_value("value1".into());
            query_params.reborrow().get(1).set_key("key2".into());
            query_params.reborrow().get(1).set_value("value2".into());
            query_params.reborrow().get(2).set_key("key3".into());
            query_params.reborrow().get(2).set_value("value3".into());
        }
        path_client = request.send().promise.await?.get()?.get_result()?;
        let mut request = path_client.post_request();
        {
            request
                .get()
                .set_body("Here's something I post. It should be returned to me".into());
        }
        let result = request.send().promise.await?;
        let result = result.get()?.get_result()?;

        let body = result.get_body()?.to_str()?;
        let body_json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(
            body_json["args"],
            serde_json::json!({"key1": "value1", "key2": "value2", "key3": "value3"})
        );
        assert_eq!(body_json["headers"]["Host"], "httpbin.org");
        assert_eq!(
            body_json["data"],
            "Here's something I post. It should be returned to me"
        );
        assert_eq!(
            body_json["url"],
            "https://httpbin.org/post?key1=value1&key2=value2&key3=value3"
        );
        /*
        let response_headers = result.get_headers()?;
        for response_header in response_headers.iter() {
            let key = response_header.get_key()?;
            let value = response_header.get_value()?;
            println!("\tKey: {key}\n\tValue: {value}\n----------------")
        }
        */
        let status = result.get_status_code();
        assert_eq!(status, 200); // 200 OK
        Ok(())
    }

    #[tokio::test]
    async fn header_test() -> capnp::Result<()> {
        // Current way to run it and see results: cargo test -- --nocapture
        let connector = HttpsConnector::new();
        let https_client = HttpClient::builder(TokioExecutor::new()).build::<_, String>(connector);
        let mut path_client: Path::Client = capnp_rpc::new_client(PathImpl::new(
            "httpbin.org",
            vec!["headers".into()],
            https_client,
        )?);

        // Set headers
        let mut request = path_client.headers_request();
        {
            let mut headers = request.get().init_headers(2);
            headers.reborrow().get(0).set_key("Header1".into());
            headers.reborrow().get(0).set_value("Value1".into());
            headers.reborrow().get(1).set_key("Header2".into());
            headers.reborrow().get(1).set_value("Value2".into());
        }
        path_client = request.send().promise.await?.get()?.get_result()?;

        // Finalize headers
        let request = path_client.finalize_headers_request();
        path_client = request.send().promise.await?.get()?.get_result()?;

        // Test that we can't add any more
        let mut request = path_client.headers_request();
        {
            let mut headers = request.get().init_headers(1);
            headers.reborrow().get(0).set_key("IllegalHeader".into());
            headers.reborrow().get(0).set_value("IllegalValue".into());
        }
        assert!(request.send().promise.await.is_err());

        let request = path_client.get_request();
        let result = request.send().promise.await?;
        let result = result.get()?.get_result()?;

        let body = result.get_body()?.to_str()?;

        let body_json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(body_json["headers"]["Header1"], "Value1");
        assert_eq!(body_json["headers"].get("IllegalHeader"), None);
        let status = result.get_status_code();
        assert_eq!(status, 200); // 200 OK
        Ok(())
    }
}
