@0xf0626a0823ecf8e0;

using import "std/byte_stream.capnp".ByteStream;
using Program = import "spawn.capnp".Program;
using ReadableMemoryBuffer = import "std/buffer.capnp".ReadableMemoryBuffer;
using PosixArgs = import "posix.capnp".PosixArgs;

struct WasmError {
    #TODO
}

interface WasmWasiProgram {
    make @0 (code :ReadableMemoryBuffer) -> (result :Program(PosixArgs, ByteStream, WasmError));
}