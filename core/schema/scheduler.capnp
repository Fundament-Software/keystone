@0x94ea6962383b011a;

interface CreateScheduler {
  create @0 () -> (scheduler :Scheduler);
}

interface Scheduler {
  repeat @0 (listener :Listener, delay :Duration) -> (id :UInt8, cancelable :Cancelable);
  scheduleFor @1 (listener :Listener, scheduleFor :UtcDateTime) -> (id :UInt8, cancelable :Cancelable);
  scheduleForThenRepeat @2 (listener :Listener, start :UtcDateTime, delay :Duration) -> (id :UInt8, cancelable :Cancelable);
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

struct UtcDateTime {
  year @0 :Int32;
  month @1 :UInt32;
  day @2 :UInt32;
  hour @3 :UInt32;
  min @4 :UInt32;
}

interface ListenerTest {
  doStuff @0 (test :UInt8) -> (result :UInt8);
}