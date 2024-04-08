@0xbffb60321ec097ca;

using import "std/byte_stream.capnp".ByteStream;
using Program = import "spawn.capnp".Program;
using ModuleError = import "module.capnp".ModuleError;
using PosixArgs = import "posix_spawn.capnp".PosixArgs;

interface ModuleOfPosix {
    wrap @0 [Config, API, Error] (prog :Program(PosixArgs, ByteStream, Error)) -> (result :Program(Config, API, ModuleError(Error)));
}
