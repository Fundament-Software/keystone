mod object_file;

use std::path::PathBuf;

pub fn create_binary_file(
    contents: &[u8],
    symbol_name: &str,
) -> Option<(Vec<u8>, Option<&'static str>)> {
    crate::object_file::create_metadata_file(
        current_platform::CURRENT_PLATFORM,
        contents,
        symbol_name,
    )
}

pub fn standard(cmdpath: &PathBuf, capnp_file: &str) {
    println!("cargo::rerun-if-changed={}", capnp_file);

    let out_dir: PathBuf = std::env::var_os("OUT_DIR").unwrap().into();

    let mut cmd = capnpc::CompilerCommand::new();
    cmd.capnp_executable(cmdpath);
    cmd.file(capnp_file);

    for (key, value) in std::env::vars() {
        if key.starts_with("DEP_") && key.ends_with("_SCHEMA_DIR") {
            println!("cargo::rustc-env={key}={value}");
            cmd.import_path(value);
        }
    }

    let temp_path = out_dir.join("keystone.schema");
    cmd.raw_code_generator_request_path(temp_path.clone());
    cmd.run().expect("compiling schema");

    let contents = std::fs::read(temp_path).expect("Failed to read compiled schema!");
    let (binfile, linkopts) = crate::object_file::create_metadata_file(
        current_platform::CURRENT_PLATFORM,
        &contents[..],
        "KEYSTONE_SCHEMA",
    )
    .expect("failed to generate schema binary");

    let filename = format!("compiled_schema_{}.o", capnp_file);
    let path = out_dir.join(filename);
    std::fs::write(&path, binfile).expect("Unable to write output file");

    println!("cargo::rustc-link-arg-bins={}", path.display());
    // Prevent the symbol from being removed as unused by the linker
    if let Some(opts) = linkopts {
        println!("cargo::rustc-link-arg-bins={opts}");
    }

    let manifest: PathBuf = std::env::var_os("CARGO_MANIFEST_DIR").unwrap().into();
    println!("cargo::metadata=SCHEMA_DIR={}", manifest.display());
}
