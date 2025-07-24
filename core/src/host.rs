use crate::capnp;
use crate::database::DatabaseExt;
use crate::keystone::CapabilityServerSetExt;
use crate::keystone::CapnpResult;
use crate::keystone_capnp::host;
use crate::sqlite::SqliteDatabase;
use crate::storage_capnp::save;
use crate::storage_capnp::sturdy_ref;
use eyre::Result;
use std::{marker::PhantomData, rc::Rc};

#[derive(Clone)]
pub struct HostImpl<State> {
    // All module instances are (usually) named, so this instance_id actually corresponds to the string index for
    // a particular module instance's name.
    instance_id: u64,
    db: Rc<SqliteDatabase>,
    phantom: PhantomData<State>,
}

impl<State> HostImpl<State>
where
    State: capnp::traits::Owned,
{
    pub fn new(instance_id: u64, db: Rc<SqliteDatabase>) -> Self {
        Self {
            instance_id,
            db,
            phantom: PhantomData,
        }
    }
}

/*
#[macro_export]
macro_rules! dyn_event {
    ($lvl:ident, $($arg:tt)+) => {
        match $lvl {
            $crate::keystone_capnp::LogLevel::None => (),
            $crate::keystone_capnp::LogLevel::Trace => ::tracing::trace!($($arg)+),
            $crate::keystone_capnp::LogLevel::Debug => ::tracing::debug!($($arg)+),
            $crate::keystone_capnp::LogLevel::Info => ::tracing::info!($($arg)+),
            $crate::keystone_capnp::LogLevel::Warning => ::tracing::warn!($($arg)+),
            $crate::keystone_capnp::LogLevel::Error => ::tracing::error!($($arg)+),
        }
    };
}

#[macro_export]
macro_rules! dyn_span {
    ($lvl:ident, $($arg:tt)+) => {
        match $lvl {
            $crate::keystone_capnp::LogLevel::None => (),
            $crate::keystone_capnp::LogLevel::Trace => ::tracing::trace_span!($($arg)+),
            $crate::keystone_capnp::LogLevel::Debug => ::tracing::debug_span!($($arg)+),
            $crate::keystone_capnp::LogLevel::Info => ::tracing::info_span!($($arg)+),
            $crate::keystone_capnp::LogLevel::Warning => ::tracing::warn_span!($($arg)+),
            $crate::keystone_capnp::LogLevel::Error => ::tracing::error_span!($($arg)+),
        }
    };
}
 */

impl<State> save::Server<capnp::any_pointer::Owned> for HostImpl<State>
where
    State: capnp::traits::Owned,
    for<'a> capnp::dynamic_value::Builder<'a>: From<<State as capnp::traits::Owned>::Builder<'a>>,
{
    async fn save(
        self: Rc<Self>,
        params: save::SaveParams<capnp::any_pointer::Owned>,
        mut results: save::SaveResults<capnp::any_pointer::Owned>,
    ) -> Result<(), capnp::Error> {
        let sturdyref = crate::sturdyref::SturdyRefImpl::init(
            self.instance_id,
            params.get()?.get_data()?,
            self.db.clone(),
        )
        .await
        .to_capnp()?;

        let cap: sturdy_ref::Client<capnp::any_pointer::Owned> = self
            .db
            .sturdyref_set
            .borrow_mut()
            .new_generic_client(sturdyref);
        results.get().set_ref(cap);
        Ok(())
    }
}

impl<State> host::Server<State> for HostImpl<State>
where
    State: capnp::traits::Owned,
    for<'a> capnp::dynamic_value::Builder<'a>: From<<State as capnp::traits::Owned>::Builder<'a>>,
{
    async fn get_state(
        self: Rc<Self>,
        _: host::GetStateParams<State>,
        mut results: host::GetStateResults<State>,
    ) -> Result<(), capnp::Error> {
        let span = tracing::debug_span!("host", id = self.instance_id);
        let _enter = span.enter();
        tracing::debug!("get_state()");
        self.db
            .get_state(self.instance_id as i64, results.get().init_state().into())
            .to_capnp()?;

        Ok(())
    }

    async fn set_state(
        self: Rc<Self>,
        params: host::SetStateParams<State>,
        _: host::SetStateResults<State>,
    ) -> Result<(), capnp::Error> {
        let span = tracing::debug_span!("host", id = self.instance_id);
        let _enter = span.enter();
        tracing::debug!("set_state()");
        self.db
            .set_state(self.instance_id as i64, params.get()?.get_state()?)
            .await
            .to_capnp()?;
        Ok(())
    }

    /*async fn log(
        &self,
        params: host::LogParams<State>,
        _: host::LogResults<State>,
    ) -> Result<(), capnp::Error> {

        let params = params.get()?;
        let obj: capnp::dynamic_value::Reader = params.get_obj()?.into();
        let level = params.get_level()?;
        let span = dyn_span!(level, "[REMOTE]", id = self.module_id);
        let _enter = span.enter();
        dyn_event!(level, "{:?}", obj);

        Ok(())
    }*/
}

pub struct HostSubscriber<State: capnp::traits::Owned> {
    client: std::sync::Mutex<Box<dyn capnp::private::capability::ClientHook>>,
    marker: PhantomData<State>,
    //localset: &tokio::task::local::LocalSet
}

impl<State: capnp::traits::Owned> HostSubscriber<State> {
    pub fn new(client: Box<dyn capnp::private::capability::ClientHook>) -> Self {
        Self {
            client: std::sync::Mutex::new(client),
            marker: PhantomData,
        }
    }
}

pub struct HostWriter<State: capnp::traits::Owned> {
    level: tracing::Level,
    client: host::Client<State>,
    buf: Vec<u8>,
}

impl<'a> tracing_subscriber::fmt::writer::MakeWriter<'a>
    for HostSubscriber<capnp::any_pointer::Owned>
{
    type Writer = HostWriter<capnp::any_pointer::Owned>;

    fn make_writer(&'a self) -> Self::Writer {
        Self::Writer {
            level: tracing::Level::INFO,
            client: capnp::capability::FromClientHook::new(self.client.lock().unwrap().add_ref()),
            buf: Vec::new(),
        }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        Self::Writer {
            level: *meta.level(),
            client: capnp::capability::FromClientHook::new(self.client.lock().unwrap().add_ref()),
            buf: Vec::new(),
        }
    }
}

impl std::io::Write for HostWriter<capnp::any_pointer::Owned> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut request = self.client.log_request();
        let mut log = request.get();

        match self.level {
            tracing::Level::TRACE => log.set_level(crate::keystone_capnp::LogLevel::Trace),
            tracing::Level::DEBUG => log.set_level(crate::keystone_capnp::LogLevel::Debug),
            tracing::Level::INFO => log.set_level(crate::keystone_capnp::LogLevel::Info),
            tracing::Level::WARN => log.set_level(crate::keystone_capnp::LogLevel::Warning),
            tracing::Level::ERROR => log.set_level(crate::keystone_capnp::LogLevel::Error),
        }

        let mut builder = log.init_obj();
        builder
            .set_as(capnp::text::Builder::new(self.buf.as_mut_slice()).reborrow_as_reader())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        self.buf.clear();
        tokio::task::spawn_local(request.send().promise);
        Ok(())
    }
}
