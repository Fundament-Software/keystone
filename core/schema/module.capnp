@0xd520bf098db5b69e;

using Cell = import "storage.capnp".Cell;

annotation autocell(field) :Void;

struct ModuleError(BackingError) {
    union {
        backing @0 :BackingError;
        protocolViolation @1 :Text;
    }
}

struct ModuleArgs(Config, Aux) {
    config @0 :Config;
    aux @1 :Aux;
}

interface ModuleStart(Config, API) {
    # This is the primary bootstrap interface returned by all keystone modules, and exposes
    # the core module management functions to keystone

    start @0 (config :Config) -> (api :API);
    stop @1 () -> ();
    dump @2 () -> ();
}