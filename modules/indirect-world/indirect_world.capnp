@0xb5519033a5bc5056;

using HelloWorld = import "/hello-world/hello_world.capnp".Root;

# Here, we copy the hello world interface because we will be forwarding our calls through it
interface Root {
    struct HelloRequest {
        name @0 :Text;
    }

    struct HelloReply {
        message @0 :Text;
    }

    sayHello @0 (request: HelloRequest) -> (reply: HelloReply);
}

struct Config {
    helloWorld @0 :HelloWorld;
}
