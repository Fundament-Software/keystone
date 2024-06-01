@0xef862f668d5adcb6;

using import "module.capnp".StatefulConfig;

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

struct MyConfig {
    echoWord @0 :Text;
}

struct MyState {
    last @0 :Text;
}

using Config = StatefulConfig(MyConfig, MyState);

#const properties :ModuleProperties = (
#    friendlyName = "Stateful",
#    stateful = true,
#    spawnID = PosixExecutable,
#    spawnDesc = (
#        path = "/usr/bin/stateful",
#        arch = Native
#    )
#);