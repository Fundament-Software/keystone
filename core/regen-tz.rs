use convert_case::Casing;
use std::io::Write;
use std::{io::Read, path::PathBuf};

// This code was used to generate the timezones, but because any new timezones must be added
// without changing the ordinal of any existing enumerants, regenerating the timezones file is
// a protocol-breaking change.
/*
fn from_scratch() {
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // This is a crude, but automatic way of finding any new timezones that weren't previously added and appending them in a non-breaking way to the file
    // Read existing file if present
    let mut existing_tzs = std::collections::HashMap::new();
    let mut next_ordinal = 0;
    let schema_path = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        PathBuf::from("core/schema/tz.capnp")
    };

    let mut file_id: Option<u64> = None;
    if schema_path.exists() {
        let mut content = String::new();
        std::fs::File::open(&schema_path)
            .and_then(|mut f| f.read_to_string(&mut content))
            .unwrap();

        let first = content.lines().nth(0);
        file_id = first.map(|line| {
            u64::from_str_radix(
                line.trim()
                    .strip_prefix("@0x")
                    .unwrap()
                    .strip_suffix(";")
                    .unwrap(),
                16,
            )
            .unwrap()
        });

        // Parse existing entries
        for line in content.lines() {
            if let Some(line) = line.trim().strip_suffix(");") {
                if let Some((name, data)) = line.split_once(" @") {
                    let name = name.trim();
                    if let Some((ordinal, tzname)) = data.split_once(" $tzname(\"") {
                        let ordinal: i32 = ordinal.parse().unwrap();
                        let tzname = tzname.trim_end_matches('"');
                        existing_tzs.insert(tzname.to_string(), (name.to_string(), ordinal));
                        next_ordinal = next_ordinal.max(ordinal + 1);
                    }
                }
            }
        }
    }

    // Generate new file
    let mut f = std::fs::File::create(&schema_path).unwrap();
    writeln!(
        &mut f,
        "@0x{:x};",
        file_id.unwrap_or_else(capnpc::generate_random_id)
    )
    .unwrap();
    writeln!(&mut f, "# DO NOT RE-ORDER INDICES").unwrap();
    writeln!(&mut f, "annotation tzname(enumerant) :Text;").unwrap();
    writeln!(&mut f).unwrap();
    writeln!(&mut f, "enum Tz {{").unwrap();

    // Process all timezones
    for tz in chrono_tz::TZ_VARIANTS {
        let tz_name = tz.name();
        let entry = if let Some((name, ordinal)) = existing_tzs.get(tz_name) {
            // Use existing entry
            (name.clone(), *ordinal)
        } else {
            // Create new entry
            let name = tz_name
                .replace("/", " ")
                .replace("_", " ")
                .replace("+", "plus")
                .replace("-", "minus")
                .to_case(convert_case::Case::Camel);
            (name, next_ordinal)
        };

        writeln!(
            &mut f,
            "  {} @{} $tzname(\"{}\");",
            entry.0, entry.1, tz_name
        )
        .unwrap();

        if !existing_tzs.contains_key(tz_name) {
            next_ordinal += 1;
        }
    }

    writeln!(&mut f, "}}").unwrap();
    writeln!(&mut f).unwrap();
}
