fn main() {
    // Re-export schema environment variables
    for (key, value) in std::env::vars() {
        if key.starts_with("DEP_") && key.ends_with("_SCHEMA_DIR") {
            println!("cargo::rustc-env={key}={value}");
        }
    }
}
