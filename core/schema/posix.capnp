@0xbffb60321ec097ca;


using import "std/byte_stream.capnp".ByteStream;
using Program = import "spawn.capnp".Program;
using ModuleError = import "module.capnp".ModuleError;
using ModuleArgs = import "module.capnp".ModuleArgs;
using Dir = import "cap_std.capnp".Dir;
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

using PosixProgram = Program(PosixArgs, ByteStream, PosixError);
# A PosixProgram is just a particular specialization of a Program that takes PosixArgs and a PosixError

interface LocalPosixProgram {
    # Creates a Posix Program out of a local file
    file @0 (file :File) -> (result :PosixProgram);
}

# using ModuleProgram[Config, API, Error, Aux] = Program(ModuleArgs(Config, Aux), API, ModuleError(Error))
# using PosixModuleProgram[Config, API, Error] = ModuleProgram(Config, API, Error, Dir)
# We can't actually write this in capnp, but conceptually, PosixModuleProgram is what wrap() returns below.

interface PosixModule {
    # Takes a Posix Program and creates a Module Program (any program that takes ModuleArgs and ModuleError) out of it 

    wrap @0 [Config, API, Error] (prog :PosixProgram) -> (result :Program(ModuleArgs(Config, Dir), API, ModuleError(Error)));
}