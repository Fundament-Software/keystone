#[cfg(windows)]
use std::process::Stdio;
use std::rc::Rc;

use tokio::sync::{oneshot, watch};

use crate::capnp;
use crate::keystone::CapnpResult;
use crate::sqlite::SqliteDatabase;

#[cfg(not(windows))]
fn native_command(
    source: &cap_std::fs::File,
    current_dir: Option<cap_std::fs::Dir>,
) -> tokio::process::Command {
    use cap_std::io_lifetimes::raw::AsRawFilelike;
    use std::ffi::OsString;
    use std::str::FromStr;

    let fd = source.as_raw_filelike();
    let path = format!("/proc/{}/fd/{}", std::process::id(), fd);

    // We can't called fexecve without reimplementing the entire process handling logic, so we just do this
    let mut cmd = tokio::process::Command::new(path);
    if let Some(dir) = current_dir {
        cmd.current_dir(
            std::fs::read_link(format!(
                "/proc/{}/fd/{}",
                std::process::id(),
                dir.as_raw_filelike()
            ))
            .unwrap(),
        );
    }

    cmd
}

#[cfg(windows)]
fn native_command(
    source: &cap_std::fs::File,
    current_dir: Option<&cap_std::fs::Dir>,
) -> tokio::process::Command {
    use cap_std::io_lifetimes::raw::AsRawFilelike;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;

    let program = unsafe {
        let mut buf = [0_u16; 2048];
        let num = GetFinalPathNameByHandleW(source.as_raw_filelike(), buf.as_mut_ptr(), 2048, 0);
        OsString::from_wide(&buf[0..num as usize])
    };

    // Note: it is actually possible to call libc::wexecve on windows, but this is unreliable.
    let mut cmd = tokio::process::Command::new(program);
    if let Some(dir) = current_dir {
        let workdir = unsafe {
            let mut buf = [0_u16; 2048];
            let num = GetFinalPathNameByHandleW(dir.as_raw_filelike(), buf.as_mut_ptr(), 2048, 0);
            OsString::from_wide(&buf[0..num as usize])
        };
        cmd.current_dir(workdir);
    }
    cmd
}

#[cfg(not(windows))]
pub fn create_ipc() -> Result<(UnixListener, String, tempfile::TempDir)> {
    let random = rand::random::<u8>() as char; //TODO security and better name
    let dir = tempfile::tempdir()?;
    let pipe_name = dir
        .path()
        .join(random.to_string().as_str())
        .to_str()
        .unwrap()
        .to_owned();
    let server = UnixListener::bind(pipe_name.clone())?;
    Ok((server, pipe_name, dir))
}

#[cfg(windows)]
pub fn spawn_process<'i, I>(
    source: &cap_std::fs::File,
    args: I,
    dir: Option<&cap_std::fs::Dir>,
    log_filter: &str,
    inherit_output: bool,
    input: Stdio,
) -> capnp::Result<tokio::process::Child>
where
    I: IntoIterator<Item = capnp::Result<&'i str>>,
{
    use std::ffi::OsString;
    use std::str::FromStr;

    let argv: Vec<OsString> = args
        .into_iter()
        .map(|x| x.map(|s| OsString::from_str(s).unwrap()))
        .collect::<capnp::Result<Vec<OsString>>>()?;

    let mut cmd = native_command(source, dir);
    cmd.args(argv)
        .stdout(if inherit_output {
            Stdio::inherit()
        } else {
            Stdio::piped()
        })
        .stderr(if inherit_output {
            Stdio::inherit()
        } else {
            Stdio::piped()
        })
        .stdin(input);

    if !log_filter.is_empty() {
        cmd.env("KEYSTONE_MODULE_LOG", log_filter);
    }

    cmd.spawn().to_capnp()
}

#[cfg(windows)]
pub fn create_ipc() -> std::io::Result<(tokio::net::windows::named_pipe::NamedPipeServer, String)> {
    let random = rand::random::<u128>(); //TODO security and better name
    let pipe_name = format!("{}{random}", r"\\.\pipe\");
    let server = tokio::net::windows::named_pipe::ServerOptions::new().create(&pipe_name)?;
    Ok((server, pipe_name))
}

pub type IdCapSet =
    crate::capnp_rpc::CapabilityServerSet<IdentifierImpl, crate::spawn_capnp::identifier::Client>;

pub struct IdentifierImpl {
    name: Vec<String>,
    pub(crate) log_filter: String,
    pub(crate) pause: watch::Receiver<bool>,
    pub(crate) finisher: atomic_take::AtomicTake<oneshot::Sender<crate::ModulePair>>,
}

impl IdentifierImpl {
    pub fn new(
        name: String,
        log_filter: String,
        pause: watch::Receiver<bool>,
        finisher: oneshot::Sender<crate::ModulePair>,
    ) -> Self {
        Self {
            name: vec![name],
            log_filter,
            pause,
            finisher: finisher.into(),
        }
    }

    pub fn id(&self, db: &Rc<SqliteDatabase>) -> eyre::Result<u64> {
        use crate::database::DatabaseExt;

        db.get_string_index(&self.to_string()).map(|x| x as u64)
    }

    pub fn get_id(name: &str, db: &Rc<SqliteDatabase>) -> eyre::Result<u64> {
        use crate::database::DatabaseExt;

        db.get_string_index(name).map(|x| x as u64)
    }
}

impl std::fmt::Display for IdentifierImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(s) = self.name.first() {
            f.write_str(s)?;
        }
        for s in self.name.iter().skip(1) {
            f.write_str(".")?;
            f.write_str(s)?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for IdentifierImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.name.iter()).finish()
    }
}

impl crate::spawn_capnp::identifier::Server for IdentifierImpl {}
