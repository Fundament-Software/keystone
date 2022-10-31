@0xaa4e9cca04ccb60c;

interface Connection {
  createDatabase @0 (name :Text) -> (database :Database);
  getDatabase @1 (name :Text) -> (database :Database);
  destroyDatabase @2 (name :Text) -> (success :Bool);
}

interface Database {
  saveDocument @0 (schema: UInt64, doc :AnyPointer) -> (success :Bool);
  deleteDocument @1 (schema: UInt64, doc :AnyPointer) -> (success :Bool);
  createView @2 [T] (name: Text, query :Text) -> (success :Bool);
  queryView @3 [T] (name: Text, params :List(Text)) -> (iter : Iterator(T));
  destroyView @4 (name: Text) -> (success :Bool);
}

interface Iterator(T) {
  take @0 (count :UInt64) -> (items: Next(T));
}

struct Next(T) {
  items @0 :AnyList; # Should be List(T) but rust doesn't support this
  next @1 :Iterator(T);
}