use std::path::PathBuf;

fn main() {
    // Export schema path
    let manifest: std::path::PathBuf = std::env::var_os("CARGO_MANIFEST_DIR").unwrap().into();
    println!(
        "cargo::metadata=SCHEMA_DIR={}/core",
        manifest.parent().unwrap().display()
    );

    let out_dir: PathBuf = std::env::var_os("OUT_DIR")
        .expect("OUT_DIR is not available, are you using this inside a build script?")
        .into();

    let mut cmd = capnpc::CompilerCommand::new();
    cmd.capnp_root("::caplog::capnp");
    cmd.output_path(out_dir.join("capnp_output"));
    cmd.omnibus(out_dir.join("capnproto.rs"));

    // Add all capnp paths
    cmd.add_paths(&["schema/**/*.capnp"]).unwrap();

    // Compile keystone schema into message for embedding
    let temp_path = out_dir.join("self.schema");
    cmd.raw_code_generator_request_path(temp_path.clone());

    // Go through all the found files and emit appropriate metadata
    for file in cmd.files() {
        println!("cargo::rerun-if-changed={}", file.display());
        println!(
            "cargo::metadata=SCHEMA_PROVIDES={}",
            keystone_build::get_capnp_id(file.as_path())
        );
    }

    cmd.run().expect("compiling schema");

    // Export schema provides
    let glob = wax::Glob::new("**/*.capnp").unwrap();
    let ids = glob
        .walk("schema/")
        .flat_map(|entry| Ok::<u64, std::io::Error>(keystone_build::get_capnp_id(entry?.path())));

    let provides = ids.map(|x| x.to_string()).collect::<Vec<_>>().join(",");
    println!("cargo::metadata=SCHEMA_PROVIDES={provides}");
}
