@0x855e374bb4054d72;

interface Https {
  domain @0 () -> (domain :Domain);
  # Might need some argument or I need to lookup if there's special syntax for none
}

interface Domain {
  subdomain @0 (name :Text) -> (subdomain :Domain);
  path @1 (name :Text) -> (path :Path);
}

interface Path {
  query @0 (key :Text, value :Text) -> (path :Path);
  path @1 (name :Text) -> (path :Path);
  # Probably constructor
  subpath @2 () -> (path :Path);
  getHttp @3 () -> (response :Response);
  # renamed temporarily to avoid name clashes
}

interface Response {
# Struct is implicit
  content @0 () -> (content :Text);
}
