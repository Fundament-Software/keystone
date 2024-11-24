@0xe93040d826e8bf57;

using import "/schema/storage.capnp".Restore;
using S = import "/schema/scheduler.capnp";

interface Callback {
    run @0 (name :Text) -> (none :Void);
}

struct Storage {
    # This is the storage that our module saves capabilities with. We use a union so we can tell which capability the storage is intended for.
    storage :union {
        callback :group {
            name @0 :Text;
        }
        # If we had another capability called "fake", we'd have a second group in the union with it's data. Instead we have a stub here for demonstration purposes.
        fake :group { foobar @1 :Void; }
    }
    # If we had data shared between all our capabilities, we could put it here
}

# A Keystone module must expose an interface called Root that corresponds to the API returned by start()
interface Root extends(Restore(Storage)) {
    struct EchoTimer {
        name @0 :Text;
        time @1 :S.CalendarTime;
        callback @2 :Callback;
    }

    echoDelay @0 (request: EchoTimer) -> ();
}

struct Config {
    greeting @0 :Text;
    scheduler @1 :S.Root;
}