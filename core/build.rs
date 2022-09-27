fn main() {
    println!("cargo:rerun-if-changed=schema/example.capnp");

    process("0.10.2", &["schema"]).expect("Capnp generation failed!")
}

use anyhow::bail;
use reqwest::StatusCode;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

pub fn append_path(cmd: &mut capnpc::CompilerCommand, path: &str) -> anyhow::Result<()> {
    let meta = std::fs::metadata(path)?;

    if meta.is_dir() {
        let dir = std::fs::read_dir(path)?;
        for fileresult in dir.into_iter() {
            append_path(
                cmd,
                fileresult?.path().to_str().expect("path is not valid utf!"),
            )?;
        }
    } else {
        println!("cargo:rerun-if-changed={:?}", path);
        cmd.file(path);
    }
    Ok(())
}
pub fn process(capnp_version: &str, paths: &[&str]) -> anyhow::Result<()> {
    let target_dir = env::var("OUT_DIR").unwrap();
    let cmdpath = fetch(capnp_version, Path::new(&target_dir)).unwrap();

    let mut cmd = capnpc::CompilerCommand::new();
    cmd.capnp_executable(cmdpath);

    for path in paths.into_iter() {
        append_path(&mut cmd, path)?;
    }
    cmd.run()?;
    Ok(())
}

pub fn fetch(version: &str, target_dir: &Path) -> anyhow::Result<PathBuf> {
    let filename = get_filename("c++", version);
    let innername = get_filename("tools", version);

    let capnp_dir = target_dir.join(&filename);
    let capnp_path = capnp_dir.join(format!("{innername}/capnp.exe"));
    if capnp_path.exists() {
        println!("capnp with correct version is already installed.");
    } else {
        let url = format!("https://capnproto.org/{filename}.zip");
        println!("capnp v{version} not found, downloading from {url}");

        let response = reqwest::blocking::get(url)?;
        if response.status() != StatusCode::OK {
            bail!(
                "Error downloading release archive: {} {}",
                response.status(),
                response.text().unwrap_or_default()
            );
        }
        println!("Download successful.");

        fs::create_dir_all(&capnp_dir)?;
        let cursor = Cursor::new(response.bytes()?);
        zip_extract::extract(cursor, &capnp_dir, false)?;
        println!("Extracted archive.");

        if !capnp_path.exists() {
            bail!(
                "Extracted capnp archive, but could not find {:?}!",
                &capnp_path
            );
        }

        println!("capnp installed successfully: {:?}", &capnp_path);
    }
    println!("`capnp --version`: {}", get_version(&capnp_path).unwrap());

    Ok(capnp_path)
}

fn get_filename(category: &str, version: &str) -> String {
    let platform = if env::consts::OS == "windows" {
        "-win32"
    } else {
        ""
    };

    format!("capnproto-{category}{platform}-{version}")
}

fn get_version(executable: &Path) -> anyhow::Result<String> {
    let version = String::from_utf8(Command::new(&executable).arg("--version").output()?.stdout)?;
    Ok(version)
}
