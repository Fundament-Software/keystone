#![warn(clippy::large_futures)]

mod binary_embed;
mod buffer_allocator;
mod byte_stream;
mod cap_replacement;
mod cap_std_capnproto;
mod cell;
pub mod config;
mod database;
pub mod host;
pub mod http;
mod keystone;
mod posix_module;
mod posix_process;
mod posix_spawn;
mod proxy;
mod sqlite;
mod sturdyref;

use atomic_take::AtomicTake;
use capnp::any_pointer::Owned as any_pointer;
use capnp::capability::FromServer;
use capnp::traits::Owned;
use capnp_macros::capnproto_rpc;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use eyre::Context;
use futures_util::StreamExt;
pub use keystone::*;
use keystone_capnp::keystone_config;
use module_capnp::module_start;
use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::rc::Rc;
use tempfile::NamedTempFile;
use tokio::sync::oneshot;
use tracing_subscriber::filter::LevelFilter;

include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

impl From<keystone_capnp::LogLevel> for tracing::Level {
    fn from(val: keystone_capnp::LogLevel) -> Self {
        match val {
            keystone_capnp::LogLevel::Trace => tracing::Level::TRACE,
            keystone_capnp::LogLevel::Debug => tracing::Level::DEBUG,
            keystone_capnp::LogLevel::Info => tracing::Level::INFO,
            keystone_capnp::LogLevel::Warning => tracing::Level::WARN,
            keystone_capnp::LogLevel::Error => tracing::Level::ERROR,
        }
    }
}

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
pub struct ModuleImpl<
    Config: 'static + capnp::traits::Owned,
    Impl: 'static + Module<Config>,
    API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: FromServer<Impl>>,
> {
    bootstrap: RefCell<Option<keystone_capnp::host::Client<any_pointer>>>,
    disconnector: RefCell<Option<capnp_rpc::Disconnector<rpc_twoparty_capnp::Side>>>,
    inner: RefCell<Option<Rc<<API::Reader<'static> as FromServer<Impl>>::Dispatch>>>,
    phantom: PhantomData<Config>,
    sender: AtomicTake<oneshot::Sender<()>>,
}

#[capnproto_rpc(module_start)]
impl<
        Config: 'static + capnp::traits::Owned,
        Impl: 'static + Module<Config>,
        API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: capnp::capability::FromServer<Impl>>,
    > module_start::Server<Config, API> for ModuleImpl<Config, Impl, API>
{
    async fn start(&self, config: Reader) -> capnp::Result<()> {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(());
        }
        tracing::debug!("Constructing module implementation");
        let bootstrap_ref = self.bootstrap.borrow_mut().as_ref().map(|x| x.clone());
        if let Some(bootstrap) = bootstrap_ref {
            let inner = Rc::new(API::Reader::from_server(
                Impl::new(config, bootstrap).await?,
            ));
            self.inner.borrow_mut().replace(inner.clone());
            let api: API::Reader<'_> = capnp::capability::FromClientHook::new(Box::new(
                capnp_rpc::local::Client::from_rc(inner),
            ));
            results.get().set_api(api)
        } else {
            Err(capnp::Error::failed("Bootstrap API did not exist?! Was start() called before the RPC connection was fully established?".into()))
        }
    }
    async fn stop(&self) -> capnp::Result<()> {
        tracing::debug!("Module recieved stop request");
        let r = self.disconnector.borrow_mut().take();
        if let Some(d) = r {
            let inner_ref = self.inner.borrow().as_ref().map(|x| x.clone());
            if let Some(inner) = inner_ref {
                inner.stop().await?;
            }

            d.await
        } else {
            Err(capnp::Error::from_kind(capnp::ErrorKind::Disconnected))
        }
    }
}

pub async fn start<
    Config: 'static + capnp::traits::Owned,
    Impl: 'static + Module<Config>,
    API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: capnp::capability::FromServer<Impl>>,
    T: tokio::io::AsyncRead + 'static + Unpin,
    U: tokio::io::AsyncWrite + 'static + Unpin,
>(
    reader: T,
    writer: U,
) -> eyre::Result<()> {
    tracing::info!("Module starting up...");
    let (sender, recv) = oneshot::channel::<()>();

    let server = Rc::new(module_start::Client::<Config, API>::from_server(
        ModuleImpl {
            bootstrap: None.into(),
            disconnector: None.into(),
            inner: None.into(),
            phantom: PhantomData,
            sender: AtomicTake::new(sender),
        },
    ));

    let module_client: module_start::Client<Config, API> = capnp::capability::FromClientHook::new(
        Box::new(capnp_rpc::local::Client::from_rc(server.clone())),
    );

    let network = twoparty::VatNetwork::new(
        reader,
        writer,
        rpc_twoparty_capnp::Side::Server,
        Default::default(),
    );
    let mut rpc_system = RpcSystem::new(Box::new(network), Some(module_client.clone().client));

    let borrow = server.as_ref();
    *borrow.bootstrap.borrow_mut() = Some(rpc_system.bootstrap(rpc_twoparty_capnp::Side::Client));
    *borrow.disconnector.borrow_mut() = Some(rpc_system.get_disconnector());

    tokio::task::spawn_local(async move {
        if tokio::time::timeout(tokio::time::Duration::from_secs(5), recv)
            .await
            .is_err()
        {
            eprintln!("The RPC system hasn't received a bootstrap response in 5 seconds! Did you try to start this module directly instead of from inside a keystone instance? It has to be started from inside a keystone configuration!");
        }
    });
    tracing::debug!("Spawning RPC system");
    let err = rpc_system.await;

    if let Err(e) = err {
        // Don't report disconnects as an error.
        if e.kind != ::capnp::ErrorKind::Disconnected {
            tracing::error!("RPC callback FAILED!");
            return Err(e.into());
        }
    }

    tracing::debug!("RPC callback returned successfully.");
    Ok(())
}

#[inline(always)]
pub async fn main<
    Config: 'static + capnp::traits::Owned,
    Impl: 'static + Module<Config>,
    API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: capnp::capability::FromServer<Impl>>,
>(
    future: impl Future,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::task::LocalSet::new()
        .run_until(async move {
            future.await;
            tokio::task::spawn_local(start::<
                Config,
                Impl,
                API,
                tokio::io::Stdin,
                tokio::io::Stdout,
            >(tokio::io::stdin(), tokio::io::stdout()))
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
    defaultLog = "debug"
    caplog = {{ trieFile = "{trie_escaped}", dataPrefix = "{prefix_escaped}" }}"#
    )
}

pub async fn test_create_keystone(
    message: &capnp::message::Builder<capnp::message::HeapAllocator>,
) -> eyre::Result<Keystone> {
    let mut instance = Keystone::new(
        message.get_root_as_reader::<keystone_config::Reader>()?,
        false,
    )?;

    instance
        .init(message.get_root_as_reader::<keystone_config::Reader>()?)
        .await?;

    Ok(instance)
}

pub fn test_harness<F: Future<Output = eyre::Result<()>> + 'static>(
    config: &str,
    f: impl FnOnce(capnp::message::Builder<capnp::message::HeapAllocator>) -> F + 'static,
) -> eyre::Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();

    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(config);
    config::to_capnp(&source.parse::<toml::Table>()?, msg.reborrow())?;

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
    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    if result.unwrap().unwrap().is_err() {
        panic!("Test took too long!");
    }

    Ok(())
}

#[inline]
pub async fn test_runner(instance: &mut Keystone) -> eyre::Result<()> {
    while let Some(r) = instance.next().await {
        r?;
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
pub async fn test_shutdown(instance: &mut Keystone) -> eyre::Result<()> {
    let (mut shutdown, runner) = instance.shutdown();

    tokio::try_join!(drive_stream(&mut shutdown), drive_stream(runner))?;
    Ok::<(), eyre::Report>(())
}

#[allow(clippy::unit_arg)]
pub fn test_module_harness<
    Config: 'static + capnp::traits::Owned,
    Impl: 'static + Module<Config>,
    API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: capnp::capability::FromServer<Impl>>,
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
        let (mut instance, api): (Keystone, API::Reader<'_>) =
            Keystone::init_single_module(&source, &module, server_reader, server_writer)
                .await
                .unwrap();

        tokio::select! {
            r = test_runner(&mut instance) => r,
            r = f(api) => r.wrap_err(module),
        }?;
        test_shutdown(&mut instance).await
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
