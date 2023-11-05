@0xfaf08161fa1ef10e;
using Spawn = import "spawn.capnp";

using import "spawn.capnp".Process; #is this right? Probably not.
using import "spawn.capnp".ServiceSpawn; #is this right? Probably not.

struct SealedReference {
	errorCode @0 :Int64;
}

interface ModuleSpawn(SealedReference, Module, ModuleError) {
   spawn @0 (reference: SealedReference) -> (result :Process(Module, ModuleError));
}

interface WrapProcessSpawn(SealedReference, Module, ModuleError) {
     create @0 (spawner: ServiceSpawn) -> (result :ModuleSpawn(SealedReference, Module, ModuleError));
}
