capnp_import::capnp_extract_bin!();

fn main() {
    let output_dir = commandhandle().unwrap();
    keystone_build::standard(output_dir.path().join("capnp"), &["stateful.capnp"]).unwrap();
}
