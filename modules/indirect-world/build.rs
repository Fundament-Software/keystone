capnp_import::capnp_extract_bin!();

fn main() {
    let output_dir = commandhandle().unwrap();
    let cmdpath = output_dir.path().join("capnp");
    println!("cargo:rerun-if-changed=indirect_world.capnp");

    capnpc::CompilerCommand::new()
        .capnp_executable(cmdpath)
        .file("indirect_world.capnp")
        .import_path("../../core/schema")
        .import_path("..")
        .raw_code_generator_request_path("keystone.schema")
        .run()
        .expect("compiling schema");
}
