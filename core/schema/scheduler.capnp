@0x94ea6962383b011a;

interface CreateScheduler {
  create @0 () -> (scheduler :Scheduler);
}

interface Scheduler {
  repeat @0 (listener :Listener, delay :Duration) -> (id :UInt8, cancelable :Cancelable);
}

interface Cancelable {
  cancel @0 () -> ();
}

interface Listener {
  event @0 (id :UInt8) -> ();
}

struct Duration {
  secs @0 :UInt64;
  millis @1 :UInt64;
}

interface ListenerTest {
  doStuff @0 (test :UInt8) -> (result :UInt8);
}