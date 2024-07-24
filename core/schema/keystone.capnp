@0xeef7e45ab0218bda;

using TOML = import "std/toml.capnp";
using Cell = import "storage.capnp".Cell;

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

struct CapExpr {
  union {
    moduleRef @0 :Text;
    field :group {
      base @1 :CapExpr;
      index @2 :UInt16;
    }
    method :group {
      subject @3 :CapExpr;
      interfaceId @4 :UInt64;
      methodId @5 :UInt16;
      args @6 :AnyPointer;
    }
  }
} 

struct KeystoneConfig {
  struct CapLogConfig {
    trieFile @0 :Text = "caplog.trie";
    dataPrefix @1 :Text = "caplog";
    maxFileSize @2 :UInt64 = 268435456;
    maxOpenFiles @3 :UInt64 = 10;
  }

  database @0 :Text = "keystone.sqlite";
  caplog @1 :CapLogConfig;

  struct ModuleConfig(T) {
    name @0 :Text;
    config @1 :T;
    path @2 :Text; # todo: replace with hash value once we have a store
    schema @3 :Text; # if this is left empty, keystone will try to extract it from the EXE
  }

  modules @2 :List(ModuleConfig);
  defaultLog @3 :LogLevel = warning;
  defaultCall @4 :CallLogLevel = nameOnly;
  socketName @5 :Text; # This is optional, if empty, keystone will put a socket in /run for installed daemons, or the user's home folder for a local session.
  password @6 :Text; # This is optional, an empty string correlates to no password.
  keys @7 :List(Text); # also optional, but will warn if no password or key is used.
  msTimeout @8 :UInt64 = 30000; # Maximum amount of time a module has to obey a stop command, in milliseconds.

  capTable @9 :List(CapExpr);
}

interface Host(State) {
  getState @0 () -> (state :State);
  setState @1 (state :State) -> ();
  initCell @2 [T] (id :Text) -> (result :Cell(T));
}