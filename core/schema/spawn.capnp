@0xf5e8de24e40c47c9;

interface Identifier {
    # Generic structured identifier, used for debug displays. The actual identifier name is not accessible by the process.
    extend @0 (name :Text) -> (id :Identifier);
}

interface Process(API, Error) {
    # Generic definition of a process. A process can ultimately be anything: a script, an executable, another capability.
    getApi @0 () -> (api :API);
    getError @1 () -> (result :Error);
    kill @2 () -> ();
    join @3 () -> (result :Error);
}

interface Program(Args, API, Error) {
    # Generic definition of something that can be executed to spawn a process. This is usually just a file.

    spawn @0 (args :Args, id :Identifier) -> (result :Process(API, Error));
}