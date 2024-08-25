@0xc61e6cbd395cf492;

interface LuaModuleApi {
    getMethods @0 () -> (methodNames :List(Text));
    callMethod @1 (id :UInt8) -> ();
}

interface HelloWorld {
    hi @0 (number: UInt8) -> (test: UInt8);
}

interface FakeKeystone {
    getLuaHelloWorldModule @0 () -> (hello: HelloWorld);
}