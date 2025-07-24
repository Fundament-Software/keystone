use crate::capnp::any_pointer::Owned as any_pointer;
use crate::capnp::any_pointer::Owned as cap_pointer;
use crate::capnp::capability::RemotePromise;
use crate::capnp_rpc::{self};
use crate::keystone::CapnpResult;
use crate::module_capnp::module_start;
use crate::module_capnp::{module_args, module_error};
use crate::posix::get_exit_code;
use crate::spawn_capnp::process;
use crate::spawn_capnp::program;
use crate::sqlite::SqliteDatabase;
use crate::{capnp, posix_capnp};
use eyre::Context;
use futures_util::FutureExt;
use futures_util::future::Shared;
use std::process::ExitStatus;
use std::{cell::RefCell, rc::Rc};
use tokio::io::AsyncReadExt;
use tokio::sync::OnceCell;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

pub struct PosixModuleProcessImpl<Fut: Future<Output = capnp::Result<ExitStatus>>> {
    process: Shared<Fut>,
    exit: OnceCell<ExitStatus>,
    pub(crate) bootstrap: module_start::Client<any_pointer, cap_pointer>,
    api: RemotePromise<module_start::start_results::Owned<any_pointer, cap_pointer>>,
    kill_token: CancellationToken,
}

type AnyPointerClient = process::Client<cap_pointer, module_error::Owned<any_pointer>>;

impl<Fut: Future<Output = capnp::Result<ExitStatus>>>
    process::Server<cap_pointer, module_error::Owned<any_pointer>> for PosixModuleProcessImpl<Fut>
{
    async fn get_error(
        self: Rc<Self>,
        _: process::GetErrorParams<cap_pointer, module_error::Owned<any_pointer>>,
        mut results: process::GetErrorResults<cap_pointer, module_error::Owned<any_pointer>>,
    ) -> Result<(), capnp::Error> {
        let span = tracing::trace_span!("posix_module_process");
        let _enter = span.enter();
        if let Some(code) = self.exit.get() {
            let mut builder: posix_capnp::posix_error::Builder =
                results.get().init_result().init_backing().init_as();
            builder.set_error_code(get_exit_code(code) as i64);
        }

        Ok(())
    }

    async fn kill(
        self: Rc<Self>,
        _: process::KillParams<cap_pointer, module_error::Owned<any_pointer>>,
        _: process::KillResults<cap_pointer, module_error::Owned<any_pointer>>,
    ) -> Result<(), capnp::Error> {
        let span = tracing::trace_span!("posix_module_process");
        let _enter = span.enter();
        tracing::trace!("kill()");
        self.kill_token.cancel();
        Ok(())
    }

    async fn get_api(
        self: Rc<Self>,
        _: process::GetApiParams<cap_pointer, module_error::Owned<any_pointer>>,
        mut results: process::GetApiResults<cap_pointer, module_error::Owned<any_pointer>>,
    ) -> Result<(), capnp::Error> {
        let span = tracing::trace_span!("posix_module_process");
        let _enter = span.enter();
        tracing::trace!("get_api()");
        results
            .get()
            .init_api()
            .set_as_capability(self.api.pipeline.get_api().as_cap());
        Ok(())
    }

    async fn join(
        self: Rc<Self>,
        _: process::JoinParams<cap_pointer, module_error::Owned<any_pointer>>,
        mut results: process::JoinResults<cap_pointer, module_error::Owned<any_pointer>>,
    ) -> Result<(), capnp::Error> {
        let span = tracing::trace_span!("posix_module_process");
        let _enter = span.enter();
        tracing::trace!("join()");

        let status = self.exit.get_or_try_init(|| self.process.clone()).await?;

        let mut builder: posix_capnp::posix_error::Builder =
            results.get().init_result().init_backing().init_as();
        builder.set_error_code(get_exit_code(status) as i64);
        Ok(())
    }
}

use crate::cap_std_capnp::dir;

pub struct PosixModuleProgramImpl {
    program: cap_std::fs::File,
    file_server: Rc<RefCell<crate::cap_std_capnproto::AmbientAuthorityImpl>>,
    id_set: Rc<RefCell<crate::process::IdCapSet>>,
    db: Rc<SqliteDatabase>,
    log_tx: Option<UnboundedSender<(u64, String)>>,
}

impl PosixModuleProgramImpl {
    pub fn new(
        program: cap_std::fs::File,
        file_server: Rc<RefCell<crate::cap_std_capnproto::AmbientAuthorityImpl>>,
        id_set: Rc<RefCell<crate::process::IdCapSet>>,
        db: Rc<SqliteDatabase>,
        log_tx: Option<UnboundedSender<(u64, String)>>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            program,
            file_server,
            id_set,
            db,
            log_tx,
        })
    }
}

use crate::spawn_capnp::program::{SpawnParams, SpawnResults};

fn log_capture<Input: tokio::io::AsyncRead + Unpin + 'static>(
    mut input: Input,
    id: u64,
    send: UnboundedSender<(u64, String)>,
) -> impl Future<Output = std::io::Result<()>> {
    async move {
        let mut buf = [0u8; 1024];
        loop {
            let mut line = String::new();
            loop {
                let len = input.read(&mut buf).await?;

                if len == 0 {
                    break;
                }

                // If we find a newline, write up to the newline, shift the buffer over, then break
                if let Some(n) = memchr::memchr(b'\n', &buf[..len]) {
                    if let Ok(s) = std::str::from_utf8(&buf[..n]) {
                        line += s;
                    }
                    buf.copy_within(n..len, 0);
                    break;
                } else if let Ok(s) = std::str::from_utf8(&buf[..len]) {
                    line += s;
                }
            }
            if line.is_empty() {
                break;
            }
            // This IS the log function so we can't really do anything if this errors
            let _ = send.send((id, line));
        }
        Ok::<(), std::io::Error>(())
    }
}

impl
    program::Server<
        module_args::Owned<any_pointer, dir::Owned>,
        cap_pointer,
        module_error::Owned<any_pointer>,
    > for PosixModuleProgramImpl
{
    async fn spawn(
        self: std::rc::Rc<Self>,
        params: SpawnParams<
            module_args::Owned<any_pointer, dir::Owned>,
            cap_pointer,
            module_error::Owned<any_pointer>,
        >,
        mut results: SpawnResults<
            module_args::Owned<any_pointer, dir::Owned>,
            cap_pointer,
            module_error::Owned<any_pointer>,
        >,
    ) -> Result<(), capnp::Error> {
        let span = tracing::span!(tracing::Level::DEBUG, "posix_module_program::spawn");
        let _enter = span.enter();
        let args = params.get()?.get_args()?;
        let id = params.get()?.get_id()?;

        let identifier =
            self.id_set
                .borrow_mut()
                .get_local_server(&id)
                .await
                .ok_or(capnp::Error::failed(
                    "ID associated with module wasn't from this keystone instance!".into(),
                ))?;

        let instance_id = identifier.id(&self.db).to_capnp()? as u64;

        let host: crate::keystone_capnp::host::Client<any_pointer> =
            capnp_rpc::new_client(crate::host::HostImpl::new(instance_id, self.db.clone()));

        #[cfg(not(windows))]
        let (server, backup_reserve_fd) = unsafe {
            //TODO Thread unsafe, might need a lock in the future
            use std::os::fd::FromRawFd;

            let backup_reserve_fd = libc::fcntl(4, libc::F_DUPFD_CLOEXEC, 5);
            if backup_reserve_fd < 0 {
                panic!("fcntl(4, F_DUPFD_CLOEXEC, 5) failed");
            }
            let mut socks = vec![0 as i32, 0 as i32];
            if libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                0,
                socks.as_mut_ptr(),
            ) < 0
            {
                libc::close(backup_reserve_fd);
                panic!("socketpair failed");
            }
            if libc::dup2(socks[1], 4) != 4 {
                libc::close(backup_reserve_fd);
                panic!("dup2(socks[1], 4) failed");
            }
            libc::close(socks[1]);
            let server =
                UnixStream::from_std(std::os::unix::net::UnixStream::from_raw_fd(socks[0]))
                    .unwrap();
            (server, backup_reserve_fd)
        };
        #[cfg(windows)]
        let (server, pipe_name) = crate::process::create_ipc()?;

        let workdir = if let Ok(aux) = args.get_aux() {
            self.file_server
                .borrow()
                .dir_set
                .get_local_server(&aux)
                .await
        } else {
            None
        };

        // Create the child process
        #[cfg(windows)]
        let mut child = crate::process::spawn_process(
            &self.program,
            [Ok::<&str, capnp::Error>(pipe_name.as_str())].into_iter(),
            workdir.as_ref().map(|x| &x.dir),
            &identifier.log_filter,
            self.log_tx.is_none(),
            std::process::Stdio::null(),
        )?;
        #[cfg(not(windows))]
        let mut child = crate::process::spawn_process(
            &self.program,
            [].into_iter(),
            workdir.as_ref().map(|x| &x.dir),
            &identifier.log_filter,
            self.log_tx.is_none(),
            std::process::Stdio::null(),
        )?;

        #[cfg(windows)]
        server.connect().await?;
        tracing::debug!("connecting to pipe");

        #[cfg(windows)]
        let (read, write) = tokio::io::split(server);
        #[cfg(not(windows))]
        let (read, write) = server.into_split();
        #[cfg(not(windows))]
        unsafe {
            libc::dup2(backup_reserve_fd, 4);
            libc::close(backup_reserve_fd);
        }

        let (mut rpc_system, bootstrap, disconnector, api) =
            crate::keystone::init_rpc_system(read, write, host.client.clone(), args.get_config()?)?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let streams = if let Some(tx) = self.log_tx.as_ref() {
            Some((
                log_capture(stdout.unwrap(), instance_id, tx.clone()),
                log_capture(stderr.unwrap(), instance_id, tx.clone()),
            ))
        } else {
            None
        };

        let kill_token = CancellationToken::new();
        let recv = kill_token.clone();
        let process = async move {
            let process_task = async move {
                tokio::select! {
                    result = child.wait() => { tracing::debug!("child {:?} returned {:?}", child.id(), &result); result.to_capnp() },
                    _result = recv.cancelled() => { tracing::warn!("kill signal sent to {:?}", child.id()); child.kill().await.to_capnp()?; Err(capnp::Error::failed("process killed".to_string())) },
                }
            };
            // We simply want to run the streams to completion, so we join them and ignore any errors that happen.
            let stream_task = async move {
                if let Some((stdout_fut, stderr_fut)) = streams {
                    let (_, _) = tokio::join!(stdout_fut, stderr_fut);
                }
                Ok::<(), capnp::Error>(())
            };

            // Because we ignore errors from the streams, this will only bail out early if the process itself errors.
            let (r, _) = tokio::try_join!(process_task, stream_task)?;
            Ok(r)
        }
        .shared();

        let mut process_handle = process.clone();
        let killer = kill_token.clone();

        let name = identifier.to_string();
        let mut rpc_future = async move {
            let result = tokio::select! {
                r = &mut rpc_system => {
                    if let Err(e) = r {
                        tracing::debug!("rpc_system returned error: {e}");
                        killer.cancel(); Err(e)
                    } else {
                        tracing::debug!("rpc_system returned success");
                        match process_handle.await {
                            Ok(e) => Ok::<ExitStatus, capnp::Error>(e),
                            Err(e) => Err(e).to_capnp(),
                        }
                    }
                },
                r = &mut process_handle => {
                        tracing::debug!("process handle returned before rpc_system closed");
                    let blah: capnp::Result<ExitStatus> = match r {
                        Ok(e) => {
                            if e.success() {
                                tracing::debug!("process returned success, awaiting rpc_system");
                                (&mut rpc_system).await.map(move |_| e)
                            } else {
                                tracing::debug!("process returned {e}, disconnecting rpc_system");
                                    let _ = disconnector.await;
                                Err(e).to_capnp()
                            }
                        }
                        Err(e) => {
                                tracing::debug!("process returned {e}, disconnecting rpc_system");
                                    let _ = disconnector.await;
                            Err(e).to_capnp()
                        }
                    };
                    blah
                 }
            };

            result
                .map(|_| ())
                .wrap_err_with(move || format!("Error from {name}"))
        }
        .boxed_local();

        let mut pause_recv = identifier.pause.clone();
        let name = identifier.to_string();
        let pause_future = async move {
            loop {
                // If the RPC system, finishes, break out of the loop. If we recieve a "go ahead" signal, continue the loop. Otherwise, pause the RPC system
                tokio::select! {
                    r = &mut rpc_future => break r,
                    _ = pause_recv.changed() => if *pause_recv.borrow_and_update() { tracing::debug!("Paused {name}"); } else { continue; },
                }

                // Wait until we recieve a false value from the signaler
                while pause_recv.changed().await.is_ok() {
                    if !*pause_recv.borrow_and_update() {
                        tracing::debug!("Unpaused {name}");
                        break;
                    }
                }
            }
        };

        if let Some(v) = identifier.finisher.take() {
            v.send((bootstrap.clone(), pause_future.boxed_local()))
                .map_err(|_| {
                    capnp::Error::failed("Failed to send module startup results!".into())
                })?;
        }

        let module_process = Rc::new(PosixModuleProcessImpl {
            process,
            bootstrap,
            api,
            kill_token,
            exit: Default::default(),
        });

        let module_process_client: process::Client<cap_pointer, module_error::Owned<any_pointer>> =
            capnp_rpc::new_client_from_rc(module_process);
        results.get().set_result(module_process_client);
        Ok(())
    }
}
