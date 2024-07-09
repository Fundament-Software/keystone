@0xcda86108d8c477fc;

struct DateTime {
    year @0 :UInt16 = 65535;
    month @1 :UInt8 = 255;
    day @2 :UInt8 = 255;
    hour @3 :UInt8 = 255;
    minute @4 :UInt8 = 255;
    second @5 :UInt8 = 255;
    nano @6 :UInt32 = 4294967295;
    offset @7 :Int16 = 32767;
}

struct Value {
  union {
    tomlString @0 :Text;
    int @1 :Int64;
    float @2 :Float64;
    boolean @3 :Bool;
    datetime @4 :DateTime;
    array @5 :List(Value);
    table @6 :List(KeyValue);
  }
}

struct KeyValue {
  key @0 :Text;
  value @1 :Value;
}