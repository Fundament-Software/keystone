@0x9663f4dd604afa35;

# A Keystone module must expose an interface called Root that corresponds to the API returned by start()
interface Root {
    struct HelloRequest {
        name @0 :Text;
    }

    struct HelloReply {
        message @0 :Text;
    }

    sayHello @0 (request: HelloRequest) -> (reply: HelloReply);
}

# All modules must have a struct named "Config" that keystone can look up when compiling
# the root configuration file. This is a stateless config - a stateful config is provided in stateful
struct Config {
    greeting @0 :Text;
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