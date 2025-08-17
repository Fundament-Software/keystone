use std::ffi::OsString;

const SERVICE_NAME: &str = "Keystone Service";

#[cfg(windows)]
mod windows {
    use crate::config;
    use crate::keystone::Keystone;
    use crate::keystone_capnp::keystone_config;
    use crate::service::SERVICE_NAME;
    use crate::{Cli, Commands};
    use clap::Parser;
    use eyre::eyre;
    use futures_util::StreamExt;
    use std::ffi::OsString;
    use std::{io::Read, path::Path, time::Duration};
    use tokio::sync::mpsc;
    use windows_service::service_control_handler::ServiceStatusHandle;
    use windows_service::service_dispatcher;
    use windows_service::{
        self, define_windows_service,
        service::{
            ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl,
            ServiceExitCode, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_manager::{ServiceManager, ServiceManagerAccess},
    };

    #[inline]
    async fn drive_stream_with_error(
        msg: &str,
        stream: &mut futures_util::stream::FuturesUnordered<impl Future<Output = eyre::Result<()>>>,
    ) {
        while let Some(r) = stream.next().await {
            if let Err(e) = r {
                eprintln!("{}: {}", msg, e);
            }
        }
    }

    define_windows_service!(ffi_service_main, service_main);

    fn service_main(arguments: Vec<OsString>) {
        // The entry point where execution will start on a background thread after a call to service_dispatcher::start
        if let Err(_e) = run_service(arguments) {
            // Log the errors/report them as windows events
        }
    }

    pub(crate) fn run_service(arguments: Vec<OsString>) -> eyre::Result<()> {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let event_handler = move |control_event| -> ServiceControlHandlerResult {
            match control_event {
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                ServiceControl::Stop => {
                    shutdown_tx.blocking_send(()).unwrap();
                    ServiceControlHandlerResult::NoError
                }
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };
        let cli = Cli::parse();
        let Commands::Service { run, toml, config } = cli.command else {
            todo!();
        };
        if !run {
            //If someone passes service from command line without the run flag, start the service normally with the args provided by install
            start_service(&[])?;
            return Ok(());
        }
        let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

        if let Some(p) = toml {
            let path = Path::new(&p);
            let dir = path.parent().unwrap_or(Path::new(""));
            let mut f = std::fs::File::open(path)?;
            let mut buf = String::new();
            f.read_to_string(&mut buf)?;
            let mut message = caplog::capnp::message::Builder::new_default();
            let mut msg = message.init_root::<keystone_config::Builder>();
            config::to_capnp(&buf.parse::<toml::Table>()?, msg.reborrow(), dir)?;
            run_keystone(
                dir,
                message.get_root_as_reader::<keystone_config::Reader>()?,
                &status_handle,
                shutdown_rx,
            )?;
        } else if let Some(p) = config {
            let path = Path::new(&p);
            run_keystone(
                path.parent().unwrap_or(Path::new("")),
                config::message_from_file(path)?.get_root::<keystone_config::Reader>()?,
                &status_handle,
                shutdown_rx,
            )?;
        } else {
            return Err(eyre!(
                "No config passed for keystone initialization inside windows service"
            ));
        }

        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;
        Ok(())
    }
    fn run_keystone(
        dir: &Path,
        message: keystone_config::Reader<'_>,
        status_handle: &ServiceStatusHandle,
        mut shutdown_rx: tokio::sync::mpsc::Receiver<()>,
    ) -> eyre::Result<()> {
        let pool = tokio::task::LocalSet::new();

        let (mut instance, mut rpc_systems) = Keystone::new(message, false, None)?;
        let fut = pool.run_until(async move {
            instance.init(dir, message.reborrow(), &rpc_systems).await?;
            tokio::select! {
                _ = drive_stream_with_error("Module crashed!", &mut rpc_systems) => (),
                _ = shutdown_rx.recv() => (),
            };

            eprintln!("Attempting graceful shutdown...");
            let mut shutdown = instance.shutdown();

            tokio::join!(
                drive_stream_with_error("Error during shutdown!", &mut shutdown),
                drive_stream_with_error("Error during shutdown RPC!", &mut rpc_systems)
            );

            Ok::<(), eyre::Report>(())
        });
        status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: std::time::Duration::default(),
            process_id: None,
        })?;

        let runtime = tokio::runtime::Runtime::new()?;
        let result = runtime.block_on(fut);
        runtime.shutdown_timeout(std::time::Duration::from_millis(1000));
        result?;
        Ok(())
    }
    pub(crate) fn run() -> Result<(), windows_service::Error> {
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
        Ok(())
    }
    pub(crate) fn install(launch_arguments: Vec<OsString>) -> eyre::Result<()> {
        let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
        let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

        let service_info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(SERVICE_NAME),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::OnDemand,
            error_control: ServiceErrorControl::Normal,
            executable_path: ::std::env::current_exe()?.with_file_name("keystone.exe"),
            launch_arguments: launch_arguments,
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };
        let service =
            service_manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)?;
        service.set_description("Keystone service")?; //TODO description
        Ok(())
    }
    pub(crate) fn uninstall(force: bool) -> eyre::Result<()> {
        let manager_access = ServiceManagerAccess::CONNECT;
        let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

        let service_access =
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE;
        let service = service_manager.open_service(SERVICE_NAME, service_access)?;
        service.delete()?;
        let service_status = service.query_status()?;
        if force {
            let Some(pid) = service_status.process_id else {
                return Err(eyre!(
                    "Service process id needed for force kill missing or process already stopped"
                ));
            };
            std::process::Command::new("taskkill")
                .args(&["/PID", pid.to_string().as_str(), "/F"])
                .output()?;
        } else if service_status.current_state != ServiceState::Stopped {
            service.stop()?;
        }
        Ok(())
    }
    pub(crate) fn start_service(start_params: &[OsString]) -> Result<(), windows_service::Error> {
        let service_manager =
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = service_manager.open_service(SERVICE_NAME, ServiceAccess::START)?;
        service.start(start_params)
    }
}

#[cfg(not(windows))]
mod systemd {
    pub(crate) fn run() -> eyre::Result<()> {
        todo!()
    }
    pub(crate) fn install() -> eyre::Result<()> {
        todo!()
    }
    pub(crate) fn uninstall() -> eyre::Result<()> {
        todo!()
    }
    pub(crate) fn start_service() -> eyre::Result<()> {
        todo!()
    }
}

pub fn install(launch_arguments: Vec<OsString>) -> eyre::Result<()> {
    #[cfg(windows)]
    windows::install(launch_arguments)?;
    #[cfg(not(windows))]
    systemd::install()?;
    Ok(())
}

pub fn uninstall(force: bool) -> eyre::Result<()> {
    #[cfg(windows)]
    windows::uninstall(force)?;
    #[cfg(not(windows))]
    systemd::uninstall()?;
    Ok(())
}

pub fn run() -> eyre::Result<()> {
    #[cfg(windows)]
    windows::run()?;
    #[cfg(not(windows))]
    systemd::run()?;
    Ok(())
}

pub fn start_service(start_params: &[OsString]) -> eyre::Result<()> {
    #[cfg(windows)]
    windows::start_service(start_params)?;
    #[cfg(not(windows))]
    systemd::start_service()?;
    Ok(())
}
