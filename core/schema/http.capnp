@0x855e374bb4054d72;

interface Https {
  domain @0 () -> (domain :Domain);
  # Might need some argument or I need to lookup if there's special syntax for none - on that note interfaces might be structs?
}

interface Domain {
  subdomain @0 (name :Text) -> (subdomain :Domain);
  path @1 (name :Text) -> (path :Path);
}

interface Path {
  query @0 (key :Text, value :Text) -> (path :Path);
  path @1 (name :Text) -> (path :Path);
  # Is this just a getter? It might be artefact from Domain - ask about this
  subpath @2 () -> (path :Path);
  get @3 () -> (response :Response);
}

struct Response {
# This surely is a struct - Https probably as well, but leave it at this for now
  content @0 :Text;
}
