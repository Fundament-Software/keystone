fn main() {
    capnp_import::process(&["schema"]).expect("Capnp generation failed!");
}
