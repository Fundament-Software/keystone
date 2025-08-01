use keystone::http::Path;
use keystone::http::https_client;

#[tokio::test]
async fn basic_test() -> eyre::Result<()> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Requesting path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Setting query
    let mut request = path_client.query_request();
    {
        let mut values_builder = request.get().init_values(2);
        values_builder.reborrow().get(0).set_key("key1".into());
        values_builder.reborrow().get(0).set_value("val1".into());
        values_builder.reborrow().get(1).set_key("key2".into());
        values_builder.reborrow().get(1).set_value("val2".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Can request content of various forms from APIs
    // GET
    let request = path_client.get_request();
    let response = request.send().promise.await?; // We'd get "temporary value dropped while borrowed" if we didn't split it
    let response = response.get()?.get_result()?;

    // Validating results - httpbin returns json as its body
    let body = response.get_body()?.to_str()?;
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
async fn domains() -> eyre::Result<()> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = domain_client.subdomain_request();
    request.get().set_name("httpbin".into());
    let subdomain_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = subdomain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // GET
    let request = path_client.get_request();

    let response = request.send().promise.await?; // We'd get "temporary value dropped while borrowed" if we didn't split it
    let response = response.get()?.get_result()?;

    let body = response.get_body()?.to_str()?;
    let body_json: serde_json::Value = serde_json::from_str(body)?;
    assert_eq!(body_json["headers"]["Host"], "httpbin.org");
    assert_eq!(body_json["url"], "https://httpbin.org/get");

    let status = response.get_status_code();
    assert_eq!(status, 200);

    Ok(())
}

// when given a domain, can retrieve subdomains
#[tokio::test]
async fn subdomains() -> eyre::Result<()> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Can specify domains and subdomains in different calls
    let mut request = domain_client.subdomain_request();
    request.get().set_name("www".into());
    let subdomain_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = subdomain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // GET
    let request = path_client.get_request();
    let response = request.send().promise.await?; // We'd get "temporary value dropped while borrowed" if we didn't split it
    let response = response.get()?.get_result()?;

    let body = response.get_body()?.to_str()?;
    let body_json: serde_json::Value = serde_json::from_str(body)?;
    assert_eq!(body_json["headers"]["Host"], "www.httpbin.org");
    assert_eq!(body_json["url"], "https://www.httpbin.org/get");

    let status = response.get_status_code();
    assert_eq!(status, 200);

    Ok(())
}

// when domain is finalized, cannot add subdomains or change domains
#[tokio::test]
async fn finalize_domains() -> eyre::Result<(), keystone::capnp::Error> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    let request = domain_client.finalize_domain_request();
    let finalized_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = finalized_client.subdomain_request();
    {
        request.get().set_name("www".into());
    }
    let err = request.send().promise.await;
    assert!(err.is_err());
    Ok(())
}

// after a path is finalized, you can't add more paths.
#[tokio::test]
async fn finalize_paths() -> eyre::Result<(), keystone::capnp::Error> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    let request = path_client.finalize_path_request();
    let finalized_client = request.send().promise.await?.get()?.get_result()?;

    let mut request = finalized_client.path_request();
    {
        let mut text_list = request.get().init_values(2);
        text_list.set(0, "foo".into());
        text_list.set(1, "bar".into());
    }
    let err = request.send().promise.await;
    assert!(err.is_err());

    Ok(())
}

// When query strings are finalized it isn't possible to add more query bindings
#[tokio::test]
async fn finalize_query() -> eyre::Result<(), keystone::capnp::Error> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Requesting path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Setting query
    let mut request = path_client.query_request();
    {
        let mut values_builder = request.get().init_values(2);
        values_builder.reborrow().get(0).set_key("key1".into());
        values_builder.reborrow().get(0).set_value("val1".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;
    let request = path_client.finalize_query_request();
    let finalized_client = request.send().promise.await?.get()?.get_result()?;

    // Setting query again
    let mut request = finalized_client.query_request();
    {
        let mut values_builder = request.get().init_values(2);
        values_builder.reborrow().get(0).set_key("key2".into());
        values_builder.reborrow().get(0).set_value("val2".into());
    }

    let err = request.send().promise.await;
    assert!(err.is_err());

    Ok(())
}

// When an allowlist of verbs is set, it isn't possible to expand the allowlist with the set allowlist function
#[tokio::test]
async fn allowlist_test() -> eyre::Result<(), keystone::capnp::Error> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Requesting path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Allow only DELETE request
    let mut request = path_client.whitelist_verbs_request();
    {
        let mut whitelist = request.get().init_verbs(1);
        whitelist.set(0, Path::HttpVerb::Delete);
    }
    let client = request.send().promise.await?.get()?.get_result()?;

    // DELETE is valid
    let request = client.delete_request();
    assert!(request.send().promise.await.is_ok());

    // GET request fails
    let request = client.get_request();
    assert_eq!(
        request.send().promise.await.err().map(|e| e.extra),
        Some("GET is not on a whitelist and can't be executed".to_string())
    );

    // Try to overwrite to allow GET
    let mut request = client.whitelist_verbs_request();
    {
        let mut whitelist = request.get().init_verbs(1);
        whitelist.set(0, Path::HttpVerb::Get);
    }
    let client = request.send().promise.await?.get()?.get_result()?;

    // GET request fails
    let request = client.get_request();
    assert_eq!(
        request.send().promise.await.err().map(|e| e.extra),
        Some("GET is not on a whitelist and can't be executed".to_string())
    );

    // DELETE request now doesn't work
    let request = client.delete_request();
    assert_eq!(
        request.send().promise.await.err().map(|e| e.extra),
        Some("DELETE is not on a whitelist and can't be executed".to_string())
    );

    Ok(())
}

// when given a path, putting .. on the parent directory doesn't work
#[tokio::test]
async fn parent_directory_path() -> eyre::Result<()> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // 1. Getting contents of root path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "".into());
    }
    let root_path_client = request.send().promise.await?.get()?.get_result()?;
    let request = root_path_client.get_request();
    let root_response = request.send().promise.await?;
    let root_response = root_response.get()?.get_result()?;
    let root_body = root_response.get_body()?;

    // 2. Trying to get parent from existing path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;
    let mut request = path_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "..".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;
    let request = path_client.get_request();
    let other_response = request.send().promise.await?;
    let other_response = other_response.get()?.get_result()?;
    let other_body = other_response.get_body()?;

    // 3. Response bodies differ
    assert_ne!(root_body, other_body);
    Ok(())
}

// when given a path, putting / on the path to go to root doesn't work
#[tokio::test]
async fn root_directory_path() -> eyre::Result<()> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // 1. Getting contents of root path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "".into());
    }
    let root_path_client = request.send().promise.await?.get()?.get_result()?;
    let request = root_path_client.get_request();
    let root_response = request.send().promise.await?;
    let root_response = root_response.get()?.get_result()?;
    let root_body = root_response.get_body()?;

    // 2. Trying to get to root from existing path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;
    let mut request = path_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "/".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;
    let request = path_client.get_request();
    let other_response = request.send().promise.await?;
    let other_response = other_response.get()?.get_result()?;
    let other_body = other_response.get_body()?;

    // 3. Response bodies differ
    assert_ne!(root_body, other_body);
    Ok(())
}

// None of the previous are possible using HTTP escapes
#[tokio::test]
async fn escapes_path() -> eyre::Result<()> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // 1. Getting contents of root path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "".into());
    }
    let root_path_client = request.send().promise.await?.get()?.get_result()?;
    let request = root_path_client.get_request();
    let root_response = request.send().promise.await?;
    let root_response = root_response.get()?.get_result()?;
    let root_body = root_response.get_body()?;

    // 2. Appending to path, so that we don't have direct access to root path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // 3. Trying to get to root via .. using escape codes
    let mut request = path_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "%2E%2E".into());
    }
    let other_client = request.send().promise.await?.get()?.get_result()?;
    let request = other_client.get_request();
    let other_response = request.send().promise.await?;
    let other_response = other_response.get()?.get_result()?;
    let other_body = other_response.get_body()?;

    // 4. Response bodies differ
    assert_ne!(root_body, other_body);

    // 5. Trying to get to root via / using escape codes
    let mut request = path_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "%2F".into());
    }
    let other_client = request.send().promise.await?.get()?.get_result()?;
    let request = other_client.get_request();
    let other_response = request.send().promise.await?;
    let other_response = other_response.get()?.get_result()?;
    let other_body = other_response.get_body()?;

    // 6. Response bodies differ
    assert_ne!(root_body, other_body);
    Ok(())
}

// When given a path, using a protocol relative URL to escape confinement doesn't work
#[tokio::test]
async fn protocol_relative_url_path() -> eyre::Result<()> {
    let client = https_client();

    // Requesting domain
    let mut request = client.domain_request();
    {
        request.get().set_name("httpbin.org".into());
    }
    let domain_client = request.send().promise.await?.get()?.get_result()?;

    // Requesting path
    let mut request = domain_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "get".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    // Attempting to disregard get
    let mut request = path_client.path_request();
    {
        let mut values_builder = request.get().init_values(1);
        values_builder.reborrow().set(0, "/httpbin.org/post".into());
    }
    let path_client = request.send().promise.await?.get()?.get_result()?;

    let request = path_client.get_request();
    let response = request.send().promise.await?; // We'd get "temporary value dropped while borrowed" if we didn't split it
    let response = response.get()?.get_result()?;

    let status = response.get_status_code();
    assert_eq!(status, 404); // as opposed to 405 we'd get with httpbin.org/post

    Ok(())
}
