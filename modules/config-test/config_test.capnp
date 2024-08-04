@0x9663f4dd604afa35;

using import "/schema/storage.capnp".Cell;
using import "/schema/module.capnp".autocell;

struct MoreState {
    stringCell @0 :Cell(Text);
    justString @1 :Text;
}

struct MyState {
    nestedState @0 :Cell(MoreState);
}

struct Config {
    struct Nested {
        state @1 :Cell(MyState) $autocell;
        moreState @0 :Cell(MoreState);
    }

    nested @0 :Nested;
}

interface Root {
    getConfig @0 () -> (reply: Config);
}

#const properties :ModuleProperties = (
#    friendlyName = "Hello World",
#    stateful = true,
#    spawnID = PosixExecutable,
#    spawnDesc = (
#        path = "/usr/bin/hello",
#        arch = Native
#    )
#);