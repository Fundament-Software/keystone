fn main() {
    capnp_import::process(&["schema/**/*.capnp"]).expect("Capnp generation failed!");
}
