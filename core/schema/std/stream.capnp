@0x8a53087a5b3b7e02;

# Intentionally an empty struct
#
# The idea here is that systems that recieve streaming data can send an object with a `write()` method that returns a `StreamResult`.
# This becomes a `Promise<()>`. The reciever can then close the promise to signify that it's ready for more data.
#
# This is the same `StreamResult` struct found in the capnp "standard library". It is manually created here because `capnp_import` does
# not currently have a way to generate code from schemas defined in the include directory.
struct StreamResult {}
