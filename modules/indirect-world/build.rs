capnp_import::capnp_extract_bin!();

fn main() {
    for (key, value) in std::env::vars() {
        println!("[KV] {}: {}", key, value);
    }
    let output_dir = commandhandle().unwrap();
    let cmdpath = output_dir.path().join("capnp");
    keystone_build::standard(&cmdpath, "indirect_world.capnp")
}
