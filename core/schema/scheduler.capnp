@0x94ea6962383b011a;

interface GetScheduler {
  get @0 () -> (scheduler :Scheduler);
}

interface Scheduler {
  repeat @0 (listener :Listener, delay :Duration) -> (id :UInt8);
  stop @1 (id :UInt8) -> ();
}

interface Listener {
  get @0 () -> (listener :Listener);
  event @1 (id :UInt8) -> ();
}

struct Duration {
  secs @0 :UInt64;
  millis @1 :UInt64;
}

interface ListenerTest {
  doStuff @0 (test :UInt8) -> (result :UInt8);
}