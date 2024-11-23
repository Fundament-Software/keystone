@0x9be7faf044e0748a;

using ST = import "storage.capnp";
using Tz = import "tz.capnp".Tz;

struct Every {
    # Description of possible recurrence intervals (some combinations may be invalid)
    months @0 :UInt32;
    days @1 :UInt64;
    hours @2 :Int64;
    millis @3 :Int64;
}

struct CalendarTime {
    year @0 :Int64;
    month @1 :UInt8;
    day @2 :UInt16;
    hour @3 :UInt16;
    min @4 :UInt16;
    sec @5 :Int64;
    milli @6 :Int64;
    tz @7 :Tz;
}

interface Action extends(ST.Saveable(Action)) {
    run @0 () -> (none :Void);
}

interface Registration {
    cancel @0 () -> (none :Void); # cancels the registration
}

interface RepeatCallback extends(ST.Saveable(RepeatCallback)) {
    repeat @0 (time :CalendarTime) -> (next :CalendarTime);
}

enum MissBehavior {
    one @0; # Default behavior - fire just 1 of the missed events (always the most recent)
    all @1; # Fire all missed events no matter how many there are
    none @2; # Don't bother firing any missed events
}

interface Root {
    once @0 (time :CalendarTime, act :Action, fireIfMissed :Bool) -> (res :Registration); # runs action at specific point in time
    every  @1 (time :CalendarTime, repeat :Every, act :Action, missed :MissBehavior) -> (res :Registration); # runs action periodically
    complex @2 (time :CalendarTime, repeat :RepeatCallback, act :Action, missed :MissBehavior) -> (res :Registration); # runs action periodically, with any arbitrary repeat option.
}