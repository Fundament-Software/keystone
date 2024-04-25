@0xf5e8de24e40c47c9;

# Generic definition of a process. A process can ultimately be anything: a script, an executable, another capability.
interface Process(API, Error) {
    getApi @0 () -> (api :API);
    getError @1 () -> (result :Error);
    kill @2 () -> ();
    join @3 () -> (result :Error);
}

interface Program(Args, API, Error) {
    spawn @0 (args :Args) -> (result :Process(API, Error));
}