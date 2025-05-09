//! Shamelessly copied from rustc codebase:
//! https://github.com/rust-lang/rust/blob/dcca6a375bd4eddb3deea7038ebf29d02af53b48/compiler/rustc_codegen_ssa/src/back/metadata.rs#L97-L206
//! and butchered ever so slightly

use object::write::{self, StandardSegment, Symbol, SymbolSection};
use object::{
    Architecture, BinaryFormat, Endianness, FileFlags, SectionFlags, SectionKind, SymbolFlags,
    SymbolKind, SymbolScope, elf,
};

/// Returns None if the architecture is not supported
pub fn create_metadata_file(
    // formerly `create_compressed_metadata_file` in the rustc codebase
    target_triple: &str,
    contents: &[u8],
    symbol_name: &str,
) -> Option<(Vec<u8>, Option<&'static str>)> {
    let platform = platforms::Platform::find(target_triple)?;

    if platform.target_arch == platforms::Arch::Wasm32
        || platform.target_arch == platforms::Arch::Wasm64
    {
        // Start with the minimum valid WASM file
        let mut result: Vec<u8> = vec![0, b'a', b's', b'm', 1, 0, 0, 0];

        // Add the `linking` section with version 2 that rust-lld expects.
        // This is required to mark the WASM file as relocatable,
        // otherwise the linker will reject it as a non-linkable file.
        // https://github.com/WebAssembly/tool-conventions/blob/master/Linking.md
        wasm_gen::write_custom_section(&mut result, "linking", &[2]);

        wasm_gen::write_custom_section(&mut result, ".dep-v0", contents);
        return Some((result, None));
    }

    let mut file = create_object_file(platform, target_triple)?;
    let section = file.add_section(
        file.segment_name(StandardSegment::Data).to_vec(),
        b".dep-v0".to_vec(),
        SectionKind::ReadOnlyData,
    );
    if let BinaryFormat::Elf = file.format() {
        // Explicitly set no flags to avoid SHF_ALLOC default for data section.
        file.section_mut(section).flags = SectionFlags::Elf { sh_flags: 0 };
    };
    let offset = file.append_section_data(section, contents, 1);

    // For MachO and probably PE this is necessary to prevent the linker from throwing away the
    // .rustc section. For ELF this isn't necessary, but it also doesn't harm.
    file.add_symbol(Symbol {
        name: symbol_name.as_bytes().to_vec(),
        value: offset,
        size: contents.len() as u64,
        kind: SymbolKind::Data,
        scope: SymbolScope::Dynamic,
        weak: false,
        section: SymbolSection::Section(section),
        flags: SymbolFlags::None,
    });

    Some((
        file.write().unwrap(),
        Some(
            if platform.target_os == platforms::OS::MacOS
                || platform.target_os == platforms::OS::iOS
            {
                "-Wl,-u,_KEYSTONE_SCHEMA"
            } else if platform.target_env == platforms::Env::Msvc {
                "/INCLUDE:KEYSTONE_SCHEMA"
            } else {
                "-Wl,--undefined=KEYSTONE_SCHEMA"
            },
        ),
    ))
}

fn create_object_file(
    platform: &platforms::Platform,
    target_triple: &str,
) -> Option<write::Object<'static>> {
    let mut e_flags = 0;
    // This conversion evolves over time, and has some subtle logic for MIPS and RISC-V later on, that also evolves.
    // If/when uplifiting this into Cargo, we will need to extract this code from rustc and put it in the `object` crate
    // so that it could be shared between rustc and Cargo.
    let endianness = match platform.target_endian {
        platforms::Endian::Little => Endianness::Little,
        platforms::Endian::Big => Endianness::Big,
        _ => return None,
    };
    let architecture = match platform.target_arch {
        platforms::Arch::Arm => Architecture::Arm,
        platforms::Arch::AArch64 => {
            if platform.target_pointer_width == platforms::PointerWidth::U32 {
                Architecture::Aarch64_Ilp32
            } else {
                Architecture::Aarch64
            }
        }
        platforms::Arch::X86 => Architecture::I386,
        platforms::Arch::S390X => Architecture::S390x,
        platforms::Arch::Mips => {
            e_flags = elf::EF_MIPS_ARCH_32R2;
            Architecture::Mips
        }
        platforms::Arch::Mips32r6 => {
            e_flags = elf::EF_MIPS_ARCH_32R6 | elf::EF_MIPS_NAN2008;
            Architecture::Mips
        }
        platforms::Arch::Mips64 => {
            e_flags = elf::EF_MIPS_ARCH_64R2;
            Architecture::Mips64
        }
        platforms::Arch::Mips64r6 => {
            e_flags = elf::EF_MIPS_ARCH_64R6 | elf::EF_MIPS_NAN2008;
            Architecture::Mips64
        }
        platforms::Arch::X86_64 => {
            if platform.target_pointer_width == platforms::PointerWidth::U32 {
                Architecture::X86_64_X32
            } else {
                Architecture::X86_64
            }
        }
        platforms::Arch::PowerPc => Architecture::PowerPc,
        platforms::Arch::PowerPc64 => Architecture::PowerPc64,
        platforms::Arch::Riscv32 => Architecture::Riscv32,
        platforms::Arch::Riscv64 => Architecture::Riscv64,
        platforms::Arch::Sparc64 => Architecture::Sparc64,
        platforms::Arch::Loongarch64 => Architecture::LoongArch64,
        _ => return None,
    };

    let binary_format = match platform.target_os {
        platforms::OS::MacOS => BinaryFormat::MachO,
        platforms::OS::iOS => BinaryFormat::MachO,
        platforms::OS::Windows => BinaryFormat::Coff,
        _ => BinaryFormat::Elf,
    };

    let mut file = write::Object::new(binary_format, architecture, endianness);
    match architecture {
        Architecture::Mips => {
            e_flags |= elf::EF_MIPS_CPIC | elf::EF_MIPS_ABI_O32;
        }
        Architecture::Mips64 => {
            e_flags |= elf::EF_MIPS_CPIC | elf::EF_MIPS_PIC;
        }
        Architecture::Riscv32 | Architecture::Riscv64 => {
            // Source: https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/079772828bd10933d34121117a222b4cc0ee2200/riscv-elf.adoc
            let features = riscv_features(target_triple);
            // Check if compressed is enabled
            if features.contains('c') {
                e_flags |= elf::EF_RISCV_RVC;
            }

            // Select the appropriate floating-point ABI
            if features.contains('d') {
                e_flags |= elf::EF_RISCV_FLOAT_ABI_DOUBLE;
            } else if features.contains('f') {
                e_flags |= elf::EF_RISCV_FLOAT_ABI_SINGLE;
            } else {
                e_flags |= elf::EF_RISCV_FLOAT_ABI_SOFT;
            }
        }
        Architecture::LoongArch64 => {
            // Source: https://github.com/loongson/la-abi-specs/blob/release/laelf.adoc#e_flags-identifies-abi-type-and-version
            e_flags |= elf::EF_LARCH_OBJABI_V1;
            let features = loongarch_features(target_triple);

            // Select the appropriate floating-point ABI
            if features.contains('d') {
                e_flags |= elf::EF_LARCH_ABI_DOUBLE_FLOAT;
            } else if features.contains('f') {
                e_flags |= elf::EF_LARCH_ABI_SINGLE_FLOAT;
            } else {
                e_flags |= elf::EF_LARCH_ABI_SOFT_FLOAT;
            }
        }
        _ => (),
    };

    // adapted from LLVM's `MCELFObjectTargetWriter::getOSABI`
    let os_abi = match platform.target_os {
        platforms::OS::Hermit => elf::ELFOSABI_STANDALONE,
        platforms::OS::FreeBSD => elf::ELFOSABI_FREEBSD,
        platforms::OS::Solaris => elf::ELFOSABI_SOLARIS,
        _ => elf::ELFOSABI_NONE,
    };

    let abi_version = 0;
    file.flags = FileFlags::Elf {
        os_abi,
        abi_version,
        e_flags,
    };
    Some(file)
}

// This function was not present in the original rustc code, which simply used
// `sess.target.options.features`
// We do not have access to compiler internals, so we have to reimplement this function.
fn riscv_features(target_triple: &str) -> String {
    let arch = target_triple.split('-').next().unwrap();
    assert_eq!(&arch[..5], "riscv");
    let mut extensions = arch[7..].to_owned();
    if extensions.contains('g') {
        extensions.push_str("imadf");
    }
    extensions
}

// This function was not present in the original rustc code, which simply used
// `sess.target.options.features`
// We do not have access to compiler internals, so we have to reimplement this function.
fn loongarch_features(target_triple: &str) -> String {
    match target_triple {
        "loongarch64-unknown-none-softfloat" => "".to_string(),
        _ => "f,d".to_string(),
    }
}
