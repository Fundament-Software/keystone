fn main() {
    let manifest: std::path::PathBuf = std::env::var_os("CARGO_MANIFEST_DIR").unwrap().into();
    println!("cargo::metadata=SCHEMA_DIR={}", manifest.display());
}
