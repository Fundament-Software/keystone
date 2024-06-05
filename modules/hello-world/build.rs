capnp_import::capnp_extract_bin!();

fn main() {
    let output_dir = commandhandle().unwrap();
    let cmdpath = output_dir.path().join("capnp");
    keystone::keystone_build_step(&cmdpath, "hello_world.capnp")
}
