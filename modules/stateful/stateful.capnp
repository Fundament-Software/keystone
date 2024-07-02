@0xef862f668d5adcb6;

using import "/schema/storage.capnp".Cell;
using import "/schema/module.capnp".autocell;

# A Keystone module must expose an interface called Root that corresponds to the API returned by start()
interface Root {
    struct EchoRequest {
        name @0 :Text;
    }

    struct EchoReply {
        message @0 :Text;
    }

    echoLast @0 (request: EchoRequest) -> (reply: EchoReply);
}

struct MyState {
    last @0 :Text;
}

struct Config {
    echoWord @0 :Text;
    # The annotation here is supposed to mark this as being automatically filled by keystone, but we can't dynamically
    # load annotations yet, so instead the fact that it's called "state" is what gets it autofilled by keystone.
    state @1 :Cell(MyState) $autocell;
}

#const properties :ModuleProperties = (
#    friendlyName = "Stateful",
#    stateful = true,
#    spawnID = PosixExecutable,
#    spawnDesc = (
#        path = "/usr/bin/stateful",
#        arch = Native
#    )
#);