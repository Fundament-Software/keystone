fn main() {
    capnp_import::process("0.10.2", &["schema"]).expect("Capnp generation failed!");
}
