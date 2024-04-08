@0xbb91542fe5a09e8f;

using import "std/byte_stream.capnp".ByteStream;
using Program = import "spawn.capnp".Program;
using File = import "cap_std.capnp".File;

struct PosixArgs {
    args @0 :List(Text);
    stdout @1 :ByteStream;
    stderr @2 :ByteStream;
}

struct PosixError {
    errorCode @0 :Int64;
    # If the error code was a standard POSIX error then this would be the human-readable error string.
    # It is valid for this to be an empty string 
    errorMessage @1 :Text;
}

interface LocalNativeProgram {
    file @0 (file :File) -> (result :Program(PosixArgs, ByteStream, PosixError));
}