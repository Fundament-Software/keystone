capnp_import::capnp_extract_bin!();

fn main() {
    // Export schema path
    let manifest: std::path::PathBuf = std::env::var_os("CARGO_MANIFEST_DIR").unwrap().into();
    println!(
        "cargo::metadata=SCHEMA_DIR={}/core",
        manifest.parent().unwrap().display()
    );
    println!(
        "cargo::rustc-env=DEP_KEYSTONE_SCHEMA_DIR={}/core",
        manifest.parent().unwrap().display()
    );

    // Export schema provides
    let glob = wax::Glob::new("**/*.capnp").unwrap();
    let ids = glob
        .walk("schema/")
        .flat_map(|entry| Ok::<u64, std::io::Error>(keystone_build::get_capnp_id(entry?.path())));

    let provides = ids.map(|x| x.to_string()).collect::<Vec<_>>().join(",");
    println!("cargo::metadata=SCHEMA_PROVIDES={}", provides);

    // Compile keystone schema into message for embedding
    let output_dir = commandhandle().unwrap();
    let cmdpath = output_dir.path().join("capnp");
    let out_dir: std::path::PathBuf = std::env::var_os("OUT_DIR").unwrap().into();

    let mut cmd = capnpc::CompilerCommand::new();
    cmd.capnp_executable(cmdpath);
    cmd.file("schema/keystone.capnp");

    let temp_path = out_dir.join("self.schema");
    cmd.raw_code_generator_request_path(temp_path.clone());
    cmd.run().expect("compiling schema");
}
