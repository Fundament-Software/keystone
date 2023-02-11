@0x855e374bb4054d72;

interface Https {
  domain @0 (name :Text) -> (result :Domain);
}

interface Domain {
  subdomain @0 (name :Text) -> (result :Domain);
  path @1 (value :Text) -> (result :Path);
}

interface Path {
  enum HttpVerb {
    get @0;
    head @1;
    post @2;
    put @3;
    delete @4;
    options @5;
    patch @6;
  }

  struct KeyValue {
    key @0 :Text;
    value @1 :Text;
  }

  struct HttpResult {
    statusCode @0 :UInt16;
    headers @1 :List(KeyValue);
    body @2 :Text;
  }

  query @0 (values :List(KeyValue)) -> (result :Path);
  path @1 (values :List(Text)) -> (result :Path);
  get @2 () -> (result :HttpResult);
  head @3 () -> (result :HttpResult);
  post @4 (body :Text) -> (result :HttpResult);
  put @5 (body :Text) -> (result :HttpResult);
  delete @6 (body :Text) -> (result :HttpResult);
  options @7 () -> (result :HttpResult);
  patch @8 (body :Text) -> (result :HttpResult);
  finalizeQuery @9 () -> (result :Path);
  finalizePath @10 () -> (result :Path);
  whitelistVerbs @11 (verbs :List(HttpVerb)) -> (result :Path);
  headers @12 (headers: List(KeyValue)) -> (result :Path);
  finalizeHeaders @13 () -> (result :Path);
}
