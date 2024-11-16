use crate::scheduler_usage_capnp::callback;
use crate::scheduler_usage_capnp::root;
use crate::scheduler_usage_capnp::storage;
use atomic_take::AtomicTake;
use capnp::any_pointer::Owned as any_pointer;
use capnp::capability::FromClientHook;
use capnp_macros::capnproto_rpc;
use chrono::Datelike;
use chrono::Timelike;
use keystone::scheduler_capnp::action;
use keystone::storage_capnp::restore;
use keystone::storage_capnp::saveable;
use keystone::CapnpResult;
use tokio::sync::oneshot;

pub struct SchedulerUsageImpl {
    pub greeting: String,
    pub scheduler: keystone::scheduler_capnp::root::Client,
    pub sender: AtomicTake<oneshot::Sender<String>>,
    pub ks: keystone::keystone_capnp::host::Client<any_pointer>,
}

struct CallbackImpl {
    sender: AtomicTake<oneshot::Sender<String>>,
}

#[capnproto_rpc(callback)]
impl callback::Server for CallbackImpl {
    async fn run(&self, name: Reader) -> capnp::Result<()> {
        tracing::info!("callback called");
        self.sender
            .take()
            .ok_or(capnp::Error::failed("tried to run callback twice!".into()))?
            .send(name.to_string()?)
            .to_capnp()?;
        Ok(())
    }
}

struct OurActionImpl {
    callback: callback::Client,
    msg: String,
    ks: keystone::keystone_capnp::host::Client<any_pointer>,
}

#[capnproto_rpc(action)]
impl action::Server for OurActionImpl {
    async fn run(&self) -> capnp::Result<()> {
        tracing::info!("run called");
        self.callback
            .build_run_request(self.msg.clone())
            .send()
            .promise
            .await?;
        Ok(())
    }
}

#[capnproto_rpc(saveable)]
impl saveable::Server<action::Owned> for OurActionImpl {
    async fn save(&self) -> capnp::Result<()> {
        tracing::info!("save called");
        let save: keystone::storage_capnp::save::Client<any_pointer> =
            self.ks.client.clone().cast_to();
        let mut req = save.save_request();
        let builder: storage::Builder = req.get().init_data().init_as();
        builder
            .init_storage()
            .init_callback()
            .set_name(self.msg.as_str().into());

        results
            .get()
            .set_ref(req.send().pipeline.get_ref().cast_to());
        Ok(())
    }
}

#[capnproto_rpc(root)]
impl root::Server for SchedulerUsageImpl {
    async fn echo_delay(&self, request: Reader) -> capnp::Result<()> {
        tracing::debug!("echo_delay was called!");
        let name = request.get_name()?.to_str()?;
        let greet = self.greeting.as_str();
        let message = format!("{greet}, {name}!");
        let datetime = chrono::offset::Utc::now();
        let action = OurActionImpl {
            callback: capnp_rpc::new_client(CallbackImpl {
                sender: AtomicTake::empty(),
            }),
            msg: message,
            ks: self.ks.clone(),
        };

        self.scheduler
            .build_once_request(
                Some(keystone::scheduler_capnp::calendar_time::CalendarTime {
                    _year: datetime.year() as i64,
                    _month: datetime.month() as u8,
                    _day: datetime.day() as u16,
                    _hour: datetime.hour() as u16,
                    _min: datetime.minute() as u16,
                    _sec: datetime.second() as i64,
                    _milli: datetime.timestamp_subsec_millis() as i64 + 1,
                    _tz: keystone::tz_capnp::Tz::Utc,
                }),
                capnp_rpc::new_client(action),
                true,
            )
            .send()
            .promise
            .await?;
        Ok(())
    }
}

#[capnproto_rpc(restore)]
impl restore::Server<storage::Owned> for SchedulerUsageImpl {
    async fn restore(&self, data: Reader) -> Result<(), ::capnp::Error> {
        tracing::info!("restore called");

        let action = match data.get_storage().which()? {
            storage::storage::Which::Callback(s) => Ok(OurActionImpl {
                callback: capnp_rpc::new_client(CallbackImpl {
                    sender: self.sender.take().expect("master sync called twice").into(),
                }),
                msg: s.get_name()?.to_string()?,
                ks: self.ks.clone(),
            }),
            storage::storage::Which::Fake(_) => {
                Err(capnp::Error::failed("Invalid storage data".into()))
            }
        }?;

        let client: action::Client = capnp_rpc::new_client(action);
        results
            .get()
            .init_cap()
            .set_as_capability(client.client.into_client_hook());
        Ok(())
    }
}

use std::sync::LazyLock;
pub static MASTER_SYNC: LazyLock<(
    AtomicTake<oneshot::Sender<String>>,
    AtomicTake<oneshot::Receiver<String>>,
)> = LazyLock::new(|| {
    let (s, r) = oneshot::channel();
    (s.into(), r.into())
});

impl keystone::Module<crate::scheduler_usage_capnp::config::Owned> for SchedulerUsageImpl {
    async fn new(
        config: <crate::scheduler_usage_capnp::config::Owned as capnp::traits::Owned>::Reader<'_>,
        ks: keystone::keystone_capnp::host::Client<any_pointer>,
    ) -> capnp::Result<Self> {
        Ok(SchedulerUsageImpl {
            greeting: config.get_greeting()?.to_string()?,
            scheduler: config.get_scheduler()?,
            sender: AtomicTake::new(MASTER_SYNC.0.take().unwrap()),
            ks,
        })
    }
}
