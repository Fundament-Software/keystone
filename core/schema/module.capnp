@0xd520bf098db5b69e;

struct ModuleError(BackingError) {
    union {
        backing @0 :BackingError;
        protocolViolation @1 :Text;
    }
}
