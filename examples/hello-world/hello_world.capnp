@0x9663f4dd604afa35;

# A Keystone module must expose an interface called Root that corresponds to the API returned by start()
interface Root {
    struct HelloRequest {
        name @0 :Text;
    }

    struct HelloReply {
        message @0 :Text;
    }
    struct UnionTest {
        union {
            i @0 :Int8;
            j @1 :Text;
        }
    }
    struct Nested {
        req @0 :HelloRequest;
        l @1 :List(HelloReply);
        d @2 :Data;
    }

    sayHello @0 (request :HelloRequest) -> (reply: HelloReply);
    getAnInt @1 (a :Int8, b: Int8, c: Int8) -> (i :Int8);
    echo @2 (i :Int8) -> (u :UInt16);
    uni @3 (u :UnionTest) -> ();
    nest @4 (n :Nested) -> (n :Nested);
    slice @5 (s :Data) -> ();
    int @6 () -> (test :Root);
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