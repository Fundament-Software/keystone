@0xa3c0d1a04bf6fdf2; # unique file ID, generated by `capnp id`

using SturdyRef = AnyPointer;
interface Saveable {
  save @0 () -> (value :SturdyRef);
}

interface Restorer {
  restore @0 [T] (value :SturdyRef) -> (cap :T);
  delete @1 (value :SturdyRef) -> ();
}


