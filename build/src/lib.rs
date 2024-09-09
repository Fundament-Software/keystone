mod object_file;

use eyre::Result;
use std::io::BufRead;
use std::path::Path;
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

pub fn get_capnp_id(file: &std::path::Path) -> u64 {
    let f = std::fs::File::open(file).expect("failed to open valid file");
    let line = std::io::BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .next()
        .unwrap();
    let num = line
        .strip_prefix("@0x")
        .unwrap_or_else(|| panic!("{} ID missing @0x prefix", file.display()));
    let (num, _) = num.split_once('#').unwrap_or((num, ""));

    u64::from_str_radix(
        num.trim_end()
            .strip_suffix(';')
            .unwrap_or_else(|| panic!("{} ID missing ; suffix", file.display())),
        16,
    )
    .expect(format!("{} ID not valid u64", file.display()).as_str())
}

pub fn extended(
    capnp_paths: &[impl AsRef<str>],
    rust_out: impl AsRef<Path>,
    capnp_out: impl AsRef<Path>,
) -> Result<()> {
    if capnp_paths.is_empty() {
        return Err(eyre::eyre!("No search patterns for capnp files specified"));
    }

    let out_dir: PathBuf = std::env::var_os("OUT_DIR")
        .expect("OUT_DIR is not available, are you using this inside a build script?")
        .into();

    let mut cmd = capnpc::CompilerCommand::new();
    cmd.output_path(out_dir.join("capnp_output"));
    cmd.omnibus(out_dir.join(rust_out));

    // Parse all dependencies
    for (key, value) in std::env::vars() {
        if key.starts_with("DEP_") && key.ends_with("_SCHEMA_DIR") {
            cmd.import_path(value);
        } else if key.starts_with("DEP_") && key.ends_with("_SCHEMA_PROVIDES") {
            let name = key
                .strip_prefix("DEP_")
                .unwrap()
                .strip_suffix("_SCHEMA_PROVIDES")
                .unwrap()
                .to_ascii_lowercase();
            cmd.crate_provides(name, value.split(',').map(|id| id.parse::<u64>().unwrap()));
        }
    }

    // Add all capnp paths
    cmd.add_paths(capnp_paths)?;
    let temp_path = out_dir.join(capnp_out);
    cmd.raw_code_generator_request_path(temp_path.clone());

    // Go through all the found files and emit appropriate metadata
    for file in cmd.files() {
        println!("cargo::rerun-if-changed={}", file.display());
        println!(
            "cargo::metadata=SCHEMA_PROVIDES={}",
            get_capnp_id(file.as_path())
        );
    }

    cmd.run()?;

    // Given the raw code generator output in temp_path, compile it into an object
    let filename = format!(
        "compiled_schema_{}.o",
        temp_path.file_stem().unwrap_or_default().to_string_lossy()
    );
    let contents = std::fs::read(temp_path)?;
    let (binfile, linkopts) = crate::object_file::create_metadata_file(
        current_platform::CURRENT_PLATFORM,
        &contents[..],
        "KEYSTONE_SCHEMA",
    )
    .expect("failed to generate schema binary");

    let path = out_dir.join(filename);
    std::fs::write(&path, binfile)?;

    // Tell cargo to link to our compiled binary object
    println!("cargo::rustc-link-arg-bins={}", path.display());
    // Prevent the symbol from being removed as unused by the linker
    if let Some(opts) = linkopts {
        println!("cargo::rustc-link-arg-bins={opts}");
    }

    // Output current manifest directory as this packages SCHEMA_DIR
    let manifest: PathBuf = std::env::var_os("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR was not set??")
        .into();
    println!("cargo::metadata=SCHEMA_DIR={}", manifest.display());

    Ok(())
}

/// Automatically configures capnproto using standard keystone dependency propagation
/// and emits proper dependency information for this package. For more options, use
/// the `extended()` function instead. Collects compilation results in
/// `capnproto.rs`, which you can include like so:
///
/// ```ignore
/// include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));
/// ```
///
/// # Example
/// Here is an example build.rs file using this function:
/// ```ignore
/// fn main() {
///     keystone_build::standard(&["example.capnp"]).unwrap();
/// }
/// ```
pub fn standard(capnp_paths: &[&str]) -> Result<()> {
    extended(capnp_paths, "capnproto.rs", "keystone.schema")
}
