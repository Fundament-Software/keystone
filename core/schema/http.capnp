@0x855e374bb4054d72;

interface HTTPS {
  domain @0 (name :Text) -> (result :Domain)
}

interface Domain {
  subdomain @0 (name :Text) -> (result :Domain);
  path @1 (name :Text) -> (result :Path);
}

interface Path {
  enum HttpVerb {
    GET @0;
    HEAD @1;
    POST @2;
    PUT @3;
    DELETE @4;
    OPTIONS @5;
    PATCH @6;
  }

  struct KeyValue {
    key @0 :Text;
    value @1 :Text;
  }

  struct HttpResult {
    statusCode @0 :Int;
    headers @1 :List(KeyValue);
    body @2 :Text;
  }

  query @0 (values :List(KeyValue)) -> (result :Path);
  path @1 (values :List(Text)) -> (result :Path);
  getHttp @2 () -> (result :HttpResult);
  # renamed temporarily to avoid name clashes
  head @3 () -> (result :HttpResult);
  post @4 (body :Text) -> (result :HttpResult);
  put @5 (body :Text) -> (result :HttpResult);
  delete @6 (body :Text) -> (result :HttpResult);
  options @7 () -> (result :HttpResult);
  patch @8 (body :Text) -> (result :HttpResult);
  finalize_query @9 -> (result :Path);
  finalize_path @10 -> (result :Path);
  whitelist_verbs @11 (verbs :List(HttpVerb)) -> (result :Path);
  headers @12 (headers: List(KeyValue)) -> (result :Path);
  finalize_headers @13 -> (result :Path);
}
