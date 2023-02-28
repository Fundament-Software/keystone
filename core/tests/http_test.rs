use assert_fs::assert;
use keystone::http::https_client;
use keystone::http::Path;

#[tokio::test]
async fn basic_test() -> anyhow::Result<()> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org");
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Requesting path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Setting query
    let mut request = path_client.query_request();
    {
        let mut values_builder = request.get().init_values(2);
        values_builder.reborrow().get(0).set_key("key1");
        values_builder.reborrow().get(0).set_value("val1");
        values_builder.reborrow().get(1).set_key("key2");
        values_builder.reborrow().get(1).set_value("val2");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Can request content of various forms from APIs
    // GET
    let request = path_client.get_request();
    let response = request.send().promise.await?; // We'd get "temporary value dropped while borrowed" if we didn't split it
    let response = response.get()?.get_result()?;

    // Validating results - httpbin returns json as its body
    let body = response.get_body()?;
    let body_json: serde_json::Value = serde_json::from_str(body)?;
    assert_eq!(
        body_json["args"],
        serde_json::json!({"key1": "val1", "key2": "val2"})
    );
    assert_eq!(body_json["headers"]["Host"], "httpbin.org");
    assert_eq!(
        body_json["url"],
        "https://httpbin.org/get?key1=val1&key2=val2"
    );

    let status = response.get_status_code();
    assert_eq!(status, 200);

    // Can provide paths piece by piece
    // can provide query strings to a path that doesn't allow subpaths and has a whitelist of verbs.

    // Response headers
    /*
    let headers = response.get_headers()?;
    for header in headers.iter() {
        let key = header.get_key()?;
        let value = header.get_value()?;
        println!("{}: {}", key, value);
    }*/
    Ok(())
}

// When given a TLD, can retrieve a domain under that TLD.
#[tokio::test]
async fn domains() -> anyhow::Result<()> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("org");
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = domain_client.subdomain_request();
    request.get().set_name("httpbin");
    let subdomain_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = subdomain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // GET
    let request = path_client.get_request();

    let response = request.send().promise.await?; // We'd get "temporary value dropped while borrowed" if we didn't split it
    let response = response.get()?.get_result()?;

    let body = response.get_body()?;
    let body_json: serde_json::Value = serde_json::from_str(body)?;
    assert_eq!(body_json["headers"]["Host"], "httpbin.org");
    assert_eq!(body_json["url"], "https://httpbin.org/get");

    let status = response.get_status_code();
    assert_eq!(status, 200);

    Ok(())
}

// when given a domain, can retrieve subdomains
#[tokio::test]
async fn subdomains() -> anyhow::Result<()> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org");
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Can specify domains and subdomains in different calls
    let mut request = domain_client.subdomain_request();
    request.get().set_name("www");
    let subdomain_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = subdomain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // GET
    let request = path_client.get_request();
    let response = request.send().promise.await?; // We'd get "temporary value dropped while borrowed" if we didn't split it
    let response = response.get()?.get_result()?;

    let body = response.get_body()?;
    let body_json: serde_json::Value = serde_json::from_str(body)?;
    assert_eq!(body_json["headers"]["Host"], "www.httpbin.org");
    assert_eq!(body_json["url"], "https://www.httpbin.org/get");

    let status = response.get_status_code();
    assert_eq!(status, 200);

    Ok(())
}

// when domain is finalized, cannot add subdomains or change domains
#[tokio::test]
async fn finalize_domains() -> anyhow::Result<(), capnp::Error> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org");
    }
    let _ = request.send().promise.await?.get()?.get_result()?;

    ::core::result::Result::Err(::capnp::Error::unimplemented(
        "domain_finalize() not implemented!".to_string(),
    ))
}

// after a path is finalized, you can't add more paths.
#[tokio::test]
async fn finalize_paths() -> anyhow::Result<(), capnp::Error> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org");
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    let request = path_client.finalize_path_request();
    let finalized_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = finalized_client.path_request();
    {
        let mut text_list = request.get().init_values(2);
        text_list.set(0, "foo");
        text_list.set(0, "bar");
    }
    let err = request.send().promise.await;
    assert!(err.is_err());

    Ok(())
}

// When query strings are finalized it isn't possible to add more query bindings
#[tokio::test]
async fn finalize_query() -> anyhow::Result<(), capnp::Error> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org");
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Requesting path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Setting query
    let mut request = path_client.query_request();
    {
        let mut values_builder = request.get().init_values(2);
        values_builder.reborrow().get(0).set_key("key1");
        values_builder.reborrow().get(0).set_value("val1");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;
    let request = path_client.finalize_query_request();
    let finalized_client = request.send().promise.await?.get()?.get_result()?;

    // Setting query again
    let mut request = finalized_client.query_request();
    {
        let mut values_builder = request.get().init_values(2);
        values_builder.reborrow().get(0).set_key("key2");
        values_builder.reborrow().get(0).set_value("val2");
    }

    let err = request.send().promise.await;
    assert!(err.is_err());

    Ok(())
}

// When query strings are finalized it isn't possible to overwrite existing query bindings or retrieve them
#[tokio::test]
async fn retrieve_query() -> anyhow::Result<(), capnp::Error> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org");
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Requesting path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Setting query
    let mut request = path_client.query_request();
    {
        let mut values_builder = request.get().init_values(2);
        values_builder.reborrow().get(0).set_key("key1");
        values_builder.reborrow().get(0).set_value("val1");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;
    let request = path_client.finalize_query_request();
    let finalized_client = request.send().promise.await?.get()?.get_result()?;

    // Attempting to get query
    let mut request = finalized_client.query_request();
    {
        let values = request.get().get_values()?;
        assert_eq!(values.len(), 0);
    }

    Ok(())
}

// When an allowlist of verbs is set, it isn't possible to expand the allowlist with the set allowlist function
#[tokio::test]
async fn allowlist_test() -> anyhow::Result<(), capnp::Error> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org");
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Requesting path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Allow only DELETE request
    let mut request = path_client.whitelist_verbs_request();
    {
        let mut whitelist = request.get().init_verbs(1);
        whitelist.set(0, Path::HttpVerb::Delete);
    }
    let client = request.send().promise.await?.get()?.get_result()?;

    // Try to overwrite with allowing a Get command
    let mut request = client.whitelist_verbs_request();
    {
        let mut whitelist = request.get().init_verbs(1);
        whitelist.set(0, Path::HttpVerb::Get);
    }
    assert_eq!(
        request.send().promise.await.err().map(|e| e.description),
        Some(
            "Can't include GET in verb whitelist, because it's not in the original one".to_string()
        )
    );

    // Test that GET request fails
    let request = client.get_request();
    assert_eq!(
        request.send().promise.await.err().map(|e| e.description),
        Some("GET is not on a whitelist and can't be executed".to_string())
    );

    // DELETE request still works
    let request = client.delete_request();
    assert!(request.send().promise.await.is_ok());

    Ok(())
}

// when given a path it isn't possible to retrieve previous components
#[tokio::test]
async fn retrieve_path() -> anyhow::Result<(), capnp::Error> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org");
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Requesting path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get");
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Attempting to get path
    let mut request = path_client.path_request();
    {
        let values = request.get().get_values()?;
        assert_eq!(values.len(), 0);
    }

    Ok(())
}

// when given a path, putting .. on the parent directory doesn't work
// when given a path, putting / on the path to go to root doesn't work
// None of the previous are possible using HTTP escapes
// When given a path, using a protocol relative URL to escape confinement doesn't work
