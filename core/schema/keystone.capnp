@0xeef7e45ab0218bda;

using TOML = import "std/toml.capnp";
using Cell = import "storage.capnp".Cell;
using Save = import "storage.capnp".Save;

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
  # Core Keystone configuration, used by keystone during initialization to know which modules to initialize

  struct CapLogConfig {
    # A struct describing where to store keystone's immutable log

    trieFile @0 :Text = "caplog.trie";
    dataPrefix @1 :Text = "caplog";
    maxFileSize @2 :UInt64 = 268435456;
    maxOpenFiles @3 :UInt64 = 10;
  }

  database @0 :Text = "keystone.sqlite";
  caplog @1 :CapLogConfig;

  struct ModuleConfig(T) {
    # A configuration that describes a module and how to find it in either the filesystem or the keystone store.

    name @0 :Text;
    config @1 :T;
    path @2 :Text; # todo: replace with hash value once we have a store
    schema @3 :Text; # if this is left empty, keystone will try to extract it from the EXE
  }

  modules @2 :List(ModuleConfig); # List of modules to be initialized on startup . Additional modules can be started later.
  defaultLog @3 :LogLevel = warning;
  defaultCall @4 :CallLogLevel = nameOnly;
  socketName @5 :Text; # This is optional, if empty, keystone will put a socket in /run for installed daemons, or the user's home folder for a local session.
  password @6 :Text; # This is optional, an empty string correlates to no password.
  keys @7 :List(Text); # also optional, but will warn if no password or key is used.
  msTimeout @8 :UInt64 = 30000; # Maximum amount of time a module has to obey a stop command, in milliseconds.

  capTable @9 :List(CapExpr); # Stores a manual serialization of capability calls.
}

interface Root {
  # Root keystone interface, which is what the config has access to

  initCell @0 [T] (id :Text, default :T) -> (result :Cell(T));
  # initializes a new cell, if it does not exist, with the provided `default` value, or an empty value if one isn't provided.
  # Returns the new cell, or an existing one if it already existed.
}

interface Host(State) extends(Save(AnyPointer)) {
  # Per-module keystone interface, used as the bootstrap interface for each module's RPC system.

  getState @0 () -> (state :State);
  setState @1 (state :State) -> ();
  log @2 [T] (level :LogLevel, obj :T) -> ();
}

interface LogAttenuate(T) {
  delegate @0 (name :Text) -> (self :T);
}