@0x81a8f0249762b5e8; # unique file ID, generated by `capnp id`

interface Logger {
  log @0 (time :UInt64, message :Text) -> ();
  logEvent @1 (time :UInt64, event :Event, tag :UInt64) -> ();
  logStructured @2 [T] (time :UInt64, schema :UInt64, message :T) -> ();
  
  enum Event {
    unknown @0;
    createNode @1;
    destroyNode @2;
    createModule @3;
    transferModule @4;
    destroyModule @5;
    getCapability @6;
    saveCapability @7;
    loadCapability @8;
    dropCapability @9;
  }
}

