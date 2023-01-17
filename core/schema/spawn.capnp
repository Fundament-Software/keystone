@0xf5e8de24e40c47c9;

struct Program {
  union {
		binary :group {
    	path @0 :Text;
  	}
  	wasm :group {
    	path @1 :Text;
  	}
  	lua :group {
    	lua @2 :Text;
  		directory @3 :Text;
  	}
	}
}

struct ProcessError {
	code @0 :Int32;
	description @1 :Text;
}

# API should a capnproto interface description
interface Process(API) {
    geterror @0 () -> (result :ProcessError);
    getapi @1 () -> (api :API);
    kill @2 ();
}

interface ServiceSpawn(API, Args) {
    # Program is a 
    # - binary executable
    # - wasm module
    # - lua script
    # (depends on what keystone is configured to understand)
    # on fs, or passed in somehow else
    #
    # it has an entrypoint, which accepts structured arguments in the form of capnproto and returns the process's root capability (this could be anything)
    # the root capability could be:
    # - the ability to recieve from stdin, and write to stdio/stdout via a cap interface (spawning legacy linux processes)
    # - structured capabilities, such as user interface meta-information or an administrator control panel for service configuration or any kind of common-response API, or a stream of structured data output  (Keystone processes)
    # (for example, a transformer: accepts a capability to a stream of structured data, and outputs a different stream of structured data)
    
    # different alternatives should be split into different types, including parameterized types. 
    
    # Process should have a relevant error relating to why the process was not spawned - if this is not possible, it could be a null capability. 
    spawn @0 (program: Program, args: Args) -> (result :Process(API));
}
