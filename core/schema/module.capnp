@0xd520bf098db5b69e;

struct ModuleError(BackingError) {
    union {
        backing @0 :BackingError;
        protocolViolation @1 :Text;
    }
}

# This is the primary bootstrap interface returned by all keystone modules, and exposes
# the core module management functions to keystone
interface ModuleStart(Config, State, API) {
    start @0 (config :Config, state :State) -> (api :API);
    stop @1 () -> ();
    dump @2 () -> ();
}