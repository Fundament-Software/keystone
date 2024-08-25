fn main() {
    keystone_build::standard(&["lua_module.capnp", "hello_world.capnp"]).unwrap();
}
//TODO struggling to import from other module, also trying to import indirect_world.capnp fails