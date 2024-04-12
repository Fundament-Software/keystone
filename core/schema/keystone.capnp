@0xeef7e45ab0218bda;

using TOML = import "toml.capnp";

enum LogLevel {
  trace @0;
  debug @1;
  info @2;
  warning @3;
  error @4;
}

enum CallLogLevel {
  none @0;
  nameOnly @1;
  primitivesOnly @2;
  limitedContent @3;
  fullContent @4;
}

struct KeystoneConfig {
  struct CapLogConfig {
    trieFile @0 :Text;
    dataPrefix @1 :Text;
    maxFileSize @2 :UInt64;
    maxOpenFiles @3 :UInt64;
  }

  database @0 :Text;
  caplog @1 :CapLogConfig;

  struct ModuleConfig {
    path @0 :Text;
    transient @1 :Bool;
    config @2 :TOML.Value;
  }

  modules @2 :List(ModuleConfig);
  defaultLog @3 :LogLevel = warning;
  defaultCall @4 :CallLogLevel = nameOnly;
  socketName @5 :Text; # This is optional, if empty, keystone will put a socket in /run for installed daemons, or the user's home folder for a local session.
  password @6 :Text; # This is optional, an empty string correlates to no password.
  keys @7 :List(Text); # also optional, but will warn if no password or key is used.
}

interface Keystone {
  getConfig @0 [T] () -> (config :T);
  setConfig @1 [T] (config :T) -> ();
}