use std::io::Write;
use std::path::PathBuf;

fn main() {
    // This code was used to generate the timezones, but because any new timezones must be added
    // without changing the ordinal of any existing enumerants, regenerating the timezones file is
    // a protocol-breaking change. DO NOT UNCOMMENT unless you intend to make a breaking change.
    /*{
        use convert_case::Casing;

        let mut f = std::fs::File::create("schema/tz.capnp").unwrap();
        writeln!(&mut f, "@0x{:x};", capnpc::generate_random_id()).unwrap();
        writeln!(&mut f, "# DO NOT RE-ORDER INDICES").unwrap();
        writeln!(&mut f, "annotation tzname(enumerant) :Text;").unwrap();
        writeln!(&mut f).unwrap();
        writeln!(&mut f, "enum Tz {{").unwrap();

        let mut ordinal = 0;
        for tz in chrono_tz::TZ_VARIANTS {
            writeln!(
                &mut f,
                "  {} @{} $tzname(\"{}\");",
                tz.name()
                    .replace("/", " ")
                    .replace("_", " ")
                    .replace("+", "plus")
                    .replace("-", "minus")
                    .to_case(convert_case::Case::Camel),
                ordinal,
                tz.name()
            )
            .unwrap();
            ordinal += 1;
        }

        writeln!(&mut f, "}}").unwrap();
        writeln!(&mut f, "").unwrap();
    }*/

    // If so, attempt to regenerate it. If this fails (because we're in a read-only context), that's fine
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
    println!("cargo::metadata=SCHEMA_PROVIDES={}", provides);
}
