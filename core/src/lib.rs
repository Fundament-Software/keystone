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

use eyre::Result;
pub use keystone::*;
use keystone_capnp::keystone_config;
use std::future::Future;
use tempfile::NamedTempFile;
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

use capnp::any_pointer::Owned as any_pointer;
use capnp::capability::FromServer;
use capnp::traits::Owned;
use capnp_macros::capnproto_rpc;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use module_capnp::module_start;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

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

pub struct ModuleImpl<
    Config: 'static + capnp::traits::Owned,
    Impl: 'static + Module<Config>,
    API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: FromServer<Impl>>,
> {
    bootstrap: RefCell<Option<keystone_capnp::host::Client<any_pointer>>>,
    disconnector: RefCell<Option<capnp_rpc::Disconnector<rpc_twoparty_capnp::Side>>>,
    inner: RefCell<Option<Rc<<API::Reader<'static> as FromServer<Impl>>::Dispatch>>>,
    phantom: PhantomData<Config>,
}

#[capnproto_rpc(module_start)]
impl<
        Config: 'static + capnp::traits::Owned,
        Impl: 'static + Module<Config>,
        API: 'static + for<'c> capnp::traits::Owned<Reader<'c>: capnp::capability::FromServer<Impl>>,
    > module_start::Server<Config, API> for ModuleImpl<Config, Impl, API>
{
    async fn start(&self, config: Reader) -> capnp::Result<()> {
        tracing::debug!("Constructing module implementation");
        if let Some(bootstrap) = self.bootstrap.borrow_mut().as_ref() {
            let inner = Rc::new(API::Reader::from_server(
                Impl::new(config, bootstrap.clone()).await?,
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
            let result = if let Some(inner) = self.inner.borrow().as_ref() {
                inner.stop().await
            } else {
                Ok(())
            };

            d.await?;
            result
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
) -> capnp::Result<()> {
    tracing::info!("Module starting up...");

    let server = Rc::new(module_start::Client::<Config, API>::from_server(
        ModuleImpl {
            bootstrap: None.into(),
            disconnector: None.into(),
            inner: None.into(),
            phantom: PhantomData,
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

    tracing::debug!("Spawning RPC system");
    rpc_system
        .await
        .map_err(|e| capnp::Error::failed(e.to_string()))?;

    tracing::debug!("RPC callback returned");
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

    tracing::info!("Exiting module");
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

pub fn test_harness<F: Future<Output = capnp::Result<()>> + 'static>(
    config: &str,
    f: impl FnOnce(Keystone) -> F + 'static,
) -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();

    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(config);

    config::to_capnp(&source.parse::<toml::Table>()?, msg.reborrow())?;

    let mut instance = Keystone::new(
        message.get_root_as_reader::<keystone_config::Reader>()?,
        false,
    )?;

    // TODO: might be able to replace the runtime catch below with .unhandled_panic(UnhandledPanic::ShutdownRuntime) if gets stabilized
    let pool = tokio::task::LocalSet::new();
    let fut = pool.run_until(async move {
        tokio::task::spawn_local(async move {
            instance
                .init(message.get_root_as_reader::<keystone_config::Reader>()?)
                .await
                .unwrap();

            f(instance).await?;
            Ok::<(), capnp::Error>(())
        })
        .await
    });

    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(fut);
    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    result.unwrap().unwrap();

    Ok(())
}
