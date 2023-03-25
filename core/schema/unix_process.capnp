@0xe9500057921f4ada;

using import "std/byte_stream.capnp".ByteStream;

struct UnixProcessApi {
	stdin @0 :ByteStream;
}

struct UnixProcessArgs {
	argv @0 :List(Text);
	stdout @1 :ByteStream;
	stderr @2 :ByteStream;
}
