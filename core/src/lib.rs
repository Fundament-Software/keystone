#![warn(clippy::large_futures)]

pub mod binary_embed;
mod buffer_allocator;
pub mod byte_stream;
mod cap_replacement;
mod cap_std_capnproto;
mod cell;
pub mod config;
mod database;
pub mod host;
pub mod http;
mod keystone;
pub mod module;
mod posix_module;
mod posix_process;
mod posix_spawn;
pub mod proxy;
pub mod scheduler;
pub mod sqlite;
mod sturdyref;
mod util;

pub use caplog::capnp;
pub use caplog::capnp_rpc;
pub use caplog::capnp_rpc::tokio;

use atomic_take::AtomicTake;
use caplog::capnp::any_pointer::Owned as any_pointer;
use caplog::capnp::capability::FromServer;
use caplog::capnp::traits::Owned;
#[cfg(windows)]
use caplog::capnp_rpc::tokio::io::{ReadHalf, WriteHalf};
#[cfg(windows)]
use caplog::capnp_rpc::tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
#[cfg(not(windows))]
use caplog::capnp_rpc::tokio::net::{UnixListener, UnixStream};
use caplog::capnp_rpc::tokio::sync::oneshot;
use caplog::capnp_rpc::{RpcSystem, rpc_twoparty_capnp, twoparty};
use capnp_macros::capnproto_rpc;
use eyre::Context;
use futures_util::StreamExt;
pub use keystone::*;
use keystone_capnp::keystone_config;
pub use module::*;
use module_capnp::module_start;
use std::future::Future;
use std::marker::PhantomData;
#[cfg(not(windows))]
use std::os::fd::FromRawFd;
use std::rc::Rc;
use tempfile::NamedTempFile;
use tokio::sync::OnceCell;
use tracing_subscriber::filter::LevelFilter;

include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

pub fn fmt(filter: impl Into<LevelFilter>) -> impl Into<tracing::Dispatch> {
    tracing_subscriber::fmt()
        .with_max_level(filter)
        .with_writer(std::io::stderr)
        .with_ansi(true)
}

/// Trait that describes a keystone module
#[allow(async_fn_in_trait)]
pub trait Module<Config: capnp::traits::Owned>: Sized {
    async fn new(
        config: <Config as Owned>::Reader<'_>,
        bootstrap: keystone_capnp::host::Client<any_pointer>,
    ) -> capnp::Result<Self>;
    async fn stop(&self) -> capnp::Result<()> {
        Ok(())
    }
    async fn dump(&self) -> capnp::Result<()> {
        Ok(())
    }
}

#[allow(clippy::type_complexity)]
pub struct ModuleImpl<Config: 'static + capnp::traits::Owned, Impl: 'static + Module<Config>> {
    bootstrap: capnp_rpc::queued::Client,
    disconnector: AtomicTake<oneshot::Sender<()>>,
    startsignal: AtomicTake<oneshot::Sender<()>>,
    inner_impl: OnceCell<Rc<Impl>>,
    phantom: PhantomData<Config>,
}

#[capnproto_rpc(module_start)]
impl<
    Config: 'static + capnp::traits::Owned,
    Impl: 'static + Module<Config>,
    API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: capnp::capability::FromServer<Impl>>,
> module_start::Server<Config, API> for ModuleImpl<Config, Impl>
{
    async fn start(self: Rc<Self>, config: Reader) -> capnp::Result<()> {
        use capnp::capability::FromClientHook;
        use capnp::private::capability::ClientHook;

        if let Some(sender) = self.startsignal.take() {
            let _ = sender.send(());
        }

        tracing::debug!("Constructing module implementation");
        let inner = self
            .inner_impl
            .get_or_try_init(|| async {
                Ok::<Rc<Impl>, capnp::Error>(Rc::new(
                    Impl::new(
                        config,
                        keystone_capnp::host::Client::<any_pointer>::new(self.bootstrap.add_ref()),
                    )
                    .await?,
                ))
            })
            .await?;
        let api: API::Reader<'_> = capnp::capability::FromClientHook::new(Box::new(
            capnp_rpc::local::Client::new(API::Reader::from_rc(inner.clone())),
        ));
        results.get().set_api(api)
    }
    async fn stop(self: Rc<Self>) -> capnp::Result<()> {
        tracing::debug!("Module recieved stop request");
        if let Some(tx) = self.disconnector.take() {
            if let Some(inner) = self.inner_impl.get() {
                inner.stop().await?;
            }

            tx.send(())
                .map_err(|_| capnp::Error::failed("Failed to send disconnect message!".into()))
        } else {
            Err(capnp::Error::from_kind(capnp::ErrorKind::Disconnected))
        }
    }
}

impl std::fmt::Debug for keystone_capnp::host::Client<capnp::any_pointer::Owned> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "[{}]", self.client.hook.get_ptr())
    }
}

pub async fn start<
    Config: 'static + capnp::traits::Owned + Unpin,
    Impl: 'static + Module<Config>,
    API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: capnp::capability::FromServer<Impl>> + Unpin,
    T: tokio::io::AsyncRead + 'static + Unpin,
    U: tokio::io::AsyncWrite + 'static + Unpin,
>(
    reader: T,
    writer: U,
) -> eyre::Result<()> {
    tracing::info!("Module starting up...");
    let (sender, recv) = oneshot::channel::<()>();
    let (tx, rx) = oneshot::channel::<()>();

    let module_impl = Rc::new(ModuleImpl {
        bootstrap: capnp_rpc::queued::Client::new(None),
        disconnector: AtomicTake::new(tx),
        startsignal: AtomicTake::new(sender),
        inner_impl: Default::default(),
        phantom: PhantomData,
    });

    let module_client: module_start::Client<Config, API> =
        caplog::capnp_rpc::new_client_from_rc(module_impl.clone());

    let network = twoparty::VatNetwork::new(
        reader,
        writer,
        rpc_twoparty_capnp::Side::Server,
        Default::default(),
    );
    let mut rpc_system = RpcSystem::new(Box::new(network), Some(module_client.clone().client));
    let disconnector = rpc_system.get_disconnector();

    capnp_rpc::queued::ClientInner::resolve(
        &module_impl.bootstrap.inner,
        Ok(rpc_system
            .bootstrap::<keystone_capnp::host::Client<any_pointer>>(
                rpc_twoparty_capnp::Side::Client,
            )
            .client
            .hook),
    );

    tokio::task::spawn_local(async move {
        if tokio::time::timeout(tokio::time::Duration::from_secs(5), recv)
            .await
            .is_err()
        {
            tracing::error!("RPC system never got bootstrap response");
            eprintln!(
                "The RPC system hasn't received a bootstrap response in 5 seconds! Did you try to start this module directly instead of from inside a keystone instance? It has to be started from inside a keystone configuration!"
            );
        }
    });
    tracing::debug!("Spawning RPC system");

    // We install a ctrl-C handler here so we can shutdown properly when the parent process gets a ctrl-C signal
    let err = tokio::select! {
        r = &mut rpc_system => r,
        _ = rx => {
            tokio::try_join!(disconnector, rpc_system).map(|_| ())
        }
        r = tokio::signal::ctrl_c() => {
            r.expect("failed to capture ctrl-c");
            let call = module_client.stop_request().send();
            tokio::try_join!(call.promise, rpc_system).map(|_| ())
        },
    };

    if let Err(e) = err {
        // Don't report disconnects as an error.
        if e.kind != capnp::ErrorKind::Disconnected {
            tracing::error!("RPC callback FAILED!");
            return Err(e.into());
        }
    }

    tracing::debug!("RPC callback returned successfully.");
    Ok(())
}

#[inline(always)]
pub async fn main<
    Config: 'static + capnp::traits::Owned + Unpin,
    Impl: 'static + Module<Config>,
    API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: capnp::capability::FromServer<Impl>> + Unpin,
>(
    future: impl Future,
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("KEYSTONE_MODULE_LOG").is_ok() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_env(
                "KEYSTONE_MODULE_LOG",
            ))
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .init();
    }

    //On windows the first arg is the named pipe path
    #[cfg(windows)]
    let cl = ClientOptions::new().open(std::env::args().nth(1).unwrap())?;
    #[cfg(windows)]
    let (read, write) = tokio::io::split(cl);
    #[cfg(windows)]
    tokio::task::LocalSet::new()
        .run_until(async move {
            future.await;
            tokio::task::spawn_local(start::<
                Config,
                Impl,
                API,
                ReadHalf<NamedPipeClient>,
                WriteHalf<NamedPipeClient>,
            >(read, write))
            .await
        })
        .await??;
    #[cfg(not(windows))]
    let (read, write) = unsafe {
        UnixStream::from_std(std::os::unix::net::UnixStream::from_raw_fd(4))
            .unwrap()
            .into_split()
    };
    #[cfg(not(windows))]
    tokio::task::LocalSet::new()
        .run_until(async move {
            future.await;
            tokio::task::spawn_local(start::<
                Config,
                Impl,
                API,
                tokio::net::unix::OwnedReadHalf,
                tokio::net::unix::OwnedWriteHalf,
            >(read, write))
            .await
        })
        .await??;

    tracing::info!("Module exiting gracefully.");
    Ok(())
}

use tempfile::TempPath;

pub fn build_temp_config(
    temp_db: &TempPath,
    temp_log: &TempPath,
    temp_prefix: &TempPath,
) -> String {
    let escaped = temp_db.as_os_str().to_str().unwrap().replace('\\', "\\\\");
    let trie_escaped = temp_log.as_os_str().to_str().unwrap().replace('\\', "\\\\");
    let prefix_escaped = temp_prefix
        .as_os_str()
        .to_str()
        .unwrap()
        .replace('\\', "\\\\");

    format!(
        r#"
    database = "{escaped}"
    defaultLog = "none"
    caplog = {{ trieFile = "{trie_escaped}", dataPrefix = "{prefix_escaped}" }}"#
    )
}

pub async fn test_create_keystone(
    message: &capnp::message::Builder<capnp::message::HeapAllocator>,
) -> eyre::Result<(Keystone, RpcSystemSet)> {
    let (mut instance, rpc_systems) = Keystone::new(
        message.get_root_as_reader::<keystone_config::Reader>()?,
        false,
    )?;

    instance
        .init(
            &std::env::current_dir()?,
            message.get_root_as_reader::<keystone_config::Reader>()?,
            &rpc_systems,
            Keystone::passthrough_stderr,
        )
        .await?;

    Ok((instance, rpc_systems))
}

pub fn test_harness<F: Future<Output = eyre::Result<()>> + 'static>(
    config: &str,
    f: impl FnOnce(capnp::message::Builder<capnp::message::HeapAllocator>) -> F + 'static,
) -> eyre::Result<()> {
    let mut message = capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();

    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(config);
    config::to_capnp(
        &source.parse::<toml::Table>()?,
        msg.reborrow(),
        &std::env::current_dir()?,
    )?;

    // TODO: might be able to replace the runtime catch below with .unhandled_panic(UnhandledPanic::ShutdownRuntime) if gets stabilized
    let pool = tokio::task::LocalSet::new();
    let fut = pool.run_until(async move {
        tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            tokio::task::spawn_local(f(message)),
        )
        .await
    });

    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(fut);
    runtime.shutdown_timeout(std::time::Duration::from_millis(100));
    if let Err(e) = result.unwrap().unwrap() {
        panic!("{e}");
    }

    Ok(())
}

#[inline]
pub async fn drive_stream(
    stream: &mut futures_util::stream::FuturesUnordered<impl Future<Output = eyre::Result<()>>>,
) -> eyre::Result<()> {
    while let Some(r) = stream.next().await {
        r?;
    }
    Ok(())
}

#[inline]
pub async fn test_shutdown(instance: &mut Keystone, runner: &mut RpcSystemSet) -> eyre::Result<()> {
    let mut shutdown = instance.shutdown();
    tokio::try_join!(drive_stream(&mut shutdown), drive_stream(runner))?;
    Ok::<(), eyre::Report>(())
}

#[allow(clippy::unit_arg)]
pub fn test_module_harness<
    Config: 'static + capnp::traits::Owned + Unpin,
    Impl: 'static + Module<Config>,
    API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: capnp::capability::FromServer<Impl>> + Unpin,
    F: Future<Output = eyre::Result<()>> + 'static,
>(
    config: &str,
    module: &str,
    f: impl for<'a> FnOnce(API::Reader<'a>) -> F + 'static,
) -> eyre::Result<()> {
    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(config);

    let (client_writer, server_reader) = async_byte_channel::channel();
    let (server_writer, client_reader) = async_byte_channel::channel();

    let pool = tokio::task::LocalSet::new();
    let a = pool.run_until(pool.spawn_local(start::<
        Config,
        Impl,
        API,
        async_byte_channel::Receiver,
        async_byte_channel::Sender,
    >(client_reader, client_writer)));

    let module = module.to_string();
    let b = pool.run_until(pool.spawn_local(async move {
        let (mut instance, api, mut rpc_systems): (Keystone, API::Reader<'_>, RpcSystemSet) =
            Keystone::init_single_module(&source, &module, server_reader, server_writer)
                .await
                .unwrap();

        tokio::select! {
            r = drive_stream(&mut rpc_systems) => r,
            r = f(api) => r.wrap_err(module),
        }?;
        test_shutdown(&mut instance, &mut rpc_systems).await
    }));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async move {
        tokio::select! {
            r = a => r,
            r = b => r,
            r = tokio::signal::ctrl_c() => Ok(Ok(r.expect("failed to capture ctrl-c"))),
        }
    });

    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    result.unwrap().unwrap();

    Ok(())
}
