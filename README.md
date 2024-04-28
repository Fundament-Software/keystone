# keystone
Capability Secure, Distributed, Reliable and Reproducible Infrastructure

## Building
This project is currently in prototyping phase, so no end-to-end test is available yet. However, you can still build the project with `cargo build --workspace --all` and test it with `cargo test --workspace --all`, which will run through our basic module test.

## Creating a Module
Our module format is currently in flux, but it should look something like this:

```capnp
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
# the root configuration file.
struct Config {
    greeting @0 :Text;
}

# The state is called State by convention, but this is not required.
# struct State {}

# If a module has properties, they must be a capnproto constant named "properties", which uses a list of pairs containing a type-id and a struct of that type.
struct MyProperties {
  isCool @0 :Bool;
}

const myProperties :MyProperties = (isCool = true);
const properties :ModuleProperties = (name = "HelloWorld", stateless = true, extensions = [(type = 0x27ae23f, data = .myProperties)]);
```
