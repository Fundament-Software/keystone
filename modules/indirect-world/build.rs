capnp_import::capnp_extract_bin!();

const CAPNP_FILE: &'static str = "indirect_world.capnp";

fn main() {
    let output_dir = commandhandle().unwrap();
    let cmdpath = output_dir.path().join("capnp");
    println!("cargo::rerun-if-changed={}", CAPNP_FILE);

    let out_dir: std::path::PathBuf = std::env::var_os("OUT_DIR").unwrap().into();

    capnpc::CompilerCommand::new()
        .capnp_executable(cmdpath)
        .file(CAPNP_FILE)
        .import_path("../../core/schema")
        .import_path("../../")
        .raw_code_generator_request_path(out_dir.join("keystone.schema"))
        .run()
        .expect("compiling schema");

    std::fs::copy(CAPNP_FILE, out_dir.join(CAPNP_FILE)).unwrap();
}
