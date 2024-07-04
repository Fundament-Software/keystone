@0xbffb60321ec097ca;


using import "std/byte_stream.capnp".ByteStream;
using Program = import "spawn.capnp".Program;
using ModuleError = import "module.capnp".ModuleError;
using PosixArgs = import "posix_spawn.capnp".PosixArgs;
using PosixError = import "posix_spawn.capnp".PosixError;
using Dir = import "cap_std.capnp".Dir;

struct PosixModuleArgs(Config) {
    config @0 :Config;
    workdir @1 :Dir;
}

interface PosixModule {
    wrap @0 [Config, API, Error] (prog :Program(PosixArgs, ByteStream, PosixError)) -> (result :Program(PosixModuleArgs(Config), API, ModuleError(Error)));
}