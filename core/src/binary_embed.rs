use crate::keystone::Error;
use binfarce::Format;
use eyre::Result;

fn get_data(data: &[u8], range: std::ops::Range<usize>) -> Result<&[u8], Error> {
    data.get(range.clone())
        .ok_or(Error::UnexpectedEof(range.start, range.end))
}

/// Load an embedded schema file from a binary
pub fn load_deps_from_binary(data: &[u8]) -> Result<&[u8]> {
    let format = binfarce::detect_format(data);
    match binfarce::detect_format(data) {
        Format::Elf32 { byte_order } => {
            let section = binfarce::elf32::parse(data, byte_order)?
                .section_with_name(".dep-v0")?
                .ok_or(Error::NoSchemaData)?;
            Ok(get_data(data, section.range()?)?)
        }
        Format::Elf64 { byte_order } => {
            let section = binfarce::elf64::parse(data, byte_order)?
                .section_with_name(".dep-v0")?
                .ok_or(Error::NoSchemaData)?;
            Ok(get_data(data, section.range()?)?)
        }
        Format::Macho => {
            let parsed = binfarce::macho::parse(data)?;
            let section = parsed.section_with_name("__DATA", ".dep-v0")?;
            let section = section.ok_or(Error::NoSchemaData)?;
            Ok(get_data(data, section.range()?)?)
        }
        Format::PE => {
            let parsed = binfarce::pe::parse(data)?;
            let section = parsed
                .section_with_name(".dep-v0")?
                .ok_or(Error::NoSchemaData)?;
            Ok(get_data(data, section.range()?)?)
        }
        Format::Unknown => {
            if data.starts_with(b"\0asm") {
                #[cfg(feature = "wasm")]
                return Ok(get_data_wasm(data)?);
            }

            Err(Error::NotValidSchema.into())
        }
    }
}

#[cfg(feature = "wasm")]
fn get_data_wasm(mut input: &[u8]) -> Result<&[u8], Error> {
    let mut parser = wasmparser::Parser::new(0);

    // `wasmparser` relies on manually advancing the offset,
    // which potentially allows infinite loops if the logic is wrong somewhere.
    // Therefore, limit the maximum number of iterations to 10,000.
    // This is way more than any sane WASM blob will have,
    // and prevents infinite loops in case of such logic errors.
    for _i in 0..10_000 {
        // wasmparser errors are strings, so we can't reasonably convert them
        let payload = match parser
            .parse(input, true)
            .map_err(|_| Error::NotValidSchema)?
        {
            // This shouldn't be possible because `eof` is always true.
            wasmparser::Chunk::NeedMoreData(_) => return Err(Error::NotValidSchema),

            wasmparser::Chunk::Parsed { payload, consumed } => {
                // Guard against made-up "consumed" values that would cause a panic
                input = match input.get(consumed..) {
                    Some(input) => input,
                    None => return Err(Error::NotValidSchema),
                };
                payload
            }
        };

        match payload {
            wasmparser::Payload::CustomSection(reader) => {
                if reader.name() == ".dep-v0" {
                    return Ok(reader.data());
                }
            }
            // We reached the end without seeing ".dep-v0" custom section
            wasmparser::Payload::End(_) => return Err(Error::NoSchemaData),
            // ignore everything that's not a custom section
            _ => {}
        }
    }

    Err(Error::NotValidSchema.into())
}
