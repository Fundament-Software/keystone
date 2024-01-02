@0x94ea6962383b011a;

interface CreateScheduler {
  create @0 () -> (scheduler :Scheduler);
}

interface Scheduler {
  #repeat @0 (listener :Listener, delay :Duration) -> (id :UInt8, cancelable :Cancelable);
  scheduleFor @0 (listener :Listener, scheduleForUnixTimestampMillis :Int64, missedEventBehavior: MissedEventBehavior) -> (id :UInt8, cancelable :Cancelable);
  scheduleForThenRepeat @1 (listener :Listener, startUnixTimestampMillis :Int64, every :Every, missedEventBehavior: MissedEventBehavior) -> (id :UInt8, cancelable :Cancelable);
}

enum MissedEventBehavior {
  sendAll @0;
}

interface Cancelable {
  cancel @0 () -> ();
}

interface Listener {
  event @0 (id :UInt8) -> ();
}

struct UnixTimestamp {
  secs @0 :Int64;
  nanos @1 :UInt32;
}

struct Every {
  months @0 :UInt32;
  days @1 :UInt64;
  hours @2 :Int64;
  mins @3 :Int64;
  secs @4 :Int64;
  millis @5 :Int64;
  tzIdentifier @6 :Text;
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
  retrieveTestVec @1 () -> (result :Data);
}