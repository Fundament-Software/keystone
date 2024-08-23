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

# All modules must have a struct named "Config" that keystone can look up when compiling
# the root configuration file. 
struct Config {
    echoWord @0 :Text;
    # The annotation here tells keystone to automatically create a cell with the ID of our module ("stateful") and return it.
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