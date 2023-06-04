@0xf5e8de24e40c47c9;

# Generic definition of a process. A process can ultimately be anything: a script, an executable, another capability.
interface Process(API, ProcessError) {
    geterror @0 () -> (result :ProcessError);
    getapi @1 () -> (api :API);
    kill @2 ();
}

# Generic definition of a capability that lets you spawn a process or another module.
# What a program is, it's API, and what arguments it takes is ultimately implementation-defined.
interface ServiceSpawn(Program, Args, API, ProcessError) {
    # Process should have a relevant error relating to why the process was not spawned - if this is not possible, it could be a null capability. 
    spawn @0 (program: Program, args: Args) -> (result :Process(API, ProcessError));
}
