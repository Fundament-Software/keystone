@0xef862f668d5adcb6;

using import "/schema/storage.capnp".Cell;
using Sqlite = import "/schema/sqlite.capnp";

# A Keystone module must expose an interface called Root that corresponds to the API returned by start()
interface Root {
    struct EchoRequest {
        name @0 :Text;
    }

    struct EchoReply {
        message @0 :Text;
    }

    echoAlphabetical @0 (request: EchoRequest) -> (reply: EchoReply);
}

# All modules must have a struct named "Config" that keystone can look up when compiling
# the root configuration file. 
struct Config {
    outer @0 :Cell(Sqlite.TableRef);
    inner @1 :Cell(Sqlite.Table);
    sqlite @2 :Sqlite.Root;
}
