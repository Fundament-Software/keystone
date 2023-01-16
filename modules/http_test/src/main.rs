use keystone::http::https_client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
        request.get().set_name("get");
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

    // GET
    let mut request = path_client.get_request();
    let response = request.send().promise.await?; // We'd get "temporary value dropped while borrowed" if we didn't split it
    let response = response.get()?.get_result()?;
    let body = response.get_body()?;
    let status = response.get_status_code();
    let headers = response.get_headers()?;
    for header in headers.iter() {
        let key = header.get_key()?;
        let value = header.get_value()?;
    }
    println!("{}", body);

    Ok(())
}
