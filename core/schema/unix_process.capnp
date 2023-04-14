@0xe9500057921f4ada;

using import "std/byte_stream.capnp".ByteStream;
using Spawn = import "spawn.capnp";

struct UnixProcessApi {
	stdin @0 :ByteStream;
}

struct UnixProcessArgs {
	argv @0 :List(Text);
	stdout @1 :ByteStream;
	stderr @2 :ByteStream;
}

struct UnixProcessError {
	errorCode @0 :Int64;
	# If the error code was a standard POSIX error then this would be the human-readable error string.
	# It is valid for this to be an empty string 
	errorMessage @1 :Text;
}
