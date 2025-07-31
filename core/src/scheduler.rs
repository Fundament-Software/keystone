use crate::capnp::introspect::Introspect;
use capnp_macros::capnproto_rpc;
use chrono::DateTime;
use chrono::Datelike;
use chrono::TimeZone;
use chrono::Timelike;
use rusqlite::CachedStatement;
use rusqlite::OptionalExtension;

use crate::capnp;
use crate::capnp::any_pointer::Owned as any_pointer;
use crate::capnp::private::capability::ClientHook;
use crate::capnp_rpc;
use crate::database::DatabaseExt;
use crate::keystone::CapnpResult;
use crate::scheduler_capnp::MissBehavior;
use crate::sqlite::SqliteDatabase;
use atomicbox::AtomicOptionBox;
use chrono_tz::Tz;
use eyre::Result;
use futures_util::TryFutureExt;
use rusqlite::params;
use std::future::Future;
use std::rc::Rc;
use std::str::FromStr;
use tokio::sync::mpsc;
use tokio::time::Duration;
use tokio::time::Instant;

// Go through the generated capnp enum and map it to the Tz Enum.
pub fn gen_tz_map<const SIZE: usize>() -> [Tz; SIZE] {
    match crate::tz_capnp::Tz::introspect().which() {
        capnp::introspect::TypeVariant::Enum(raw_enum_schema) => {
            let schema: capnp::schema::EnumSchema = raw_enum_schema.into();
            if schema.get_enumerants().unwrap().len() as usize != SIZE {
                panic!(
                    "SIZE is incorrect, it should be {}",
                    schema.get_enumerants().unwrap().len()
                );
            }
            let mut list: [std::mem::MaybeUninit<Tz>; SIZE] =
                [const { std::mem::MaybeUninit::uninit() }; SIZE];

            for enumerant in schema.get_enumerants().unwrap().iter() {
                list[enumerant.get_ordinal() as usize].write(
                    Tz::from_str(
                        enumerant
                            .get_annotations()
                            .unwrap()
                            .get(0)
                            .get_value()
                            .unwrap()
                            .downcast::<capnp::text::Reader<'_>>()
                            .to_str()
                            .unwrap(),
                    )
                    .unwrap(),
                );
            }

            list.map(|v| unsafe { v.assume_init() })
        }
        _ => panic!("what???"),
    }
}

use std::sync::LazyLock;
static TZ_MAP: LazyLock<[Tz; 596]> = LazyLock::new(gen_tz_map);
static LAST_ERROR: AtomicOptionBox<eyre::ErrReport> = AtomicOptionBox::none();

enum Queue {
    Cancel(i64),
    Add,
}

// Trait to enable dependency injection for testing purposes
trait TimeSource {
    async fn now(&mut self) -> i64;
    async fn sleep(&mut self, deadline: Instant);
}

impl TimeSource for () {
    async fn now(&mut self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    async fn sleep(&mut self, deadline: Instant) {
        tokio::time::sleep_until(deadline).await;
    }
}

pub struct Scheduler {
    db: Rc<SqliteDatabase>,
    signal: tokio::sync::mpsc::Sender<Queue>,
}

const TIMER_EPSILON: i64 = 20; // ms

#[inline]
fn from_ymd_hms_ms(
    t: crate::scheduler_capnp::calendar_time::Reader<'_>,
    tz_index: crate::tz_capnp::Tz,
    offset: i16,
) -> capnp::Result<chrono::offset::LocalResult<DateTime<Tz>>> {
    let y = t.get_year();
    let m = t.get_month();
    let d = t.get_day();
    let h = t.get_hour() as i16 + offset;
    let min = t.get_min();
    let s = t.get_sec();
    let ms = t.get_milli();

    let tz: Tz = TZ_MAP[tz_index as usize];
    Ok(
        chrono::NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32)
            .ok_or_else(|| capnp::Error::failed("Invalid date".into()))?
            .and_hms_milli_opt(h as u32, min as u32, s as u32, ms as u32)
            .ok_or_else(|| capnp::Error::failed("Invalid time".into()))?
            .and_local_timezone(tz),
    )
}

fn get_timestamp(t: crate::scheduler_capnp::calendar_time::Reader<'_>) -> capnp::Result<i64> {
    let tz_index = t.get_tz()?;
    if let Some(t) = from_ymd_hms_ms(t, tz_index, 0)?.earliest() {
        Ok(t.timestamp_millis())
    } else if let Some(t) = from_ymd_hms_ms(t, tz_index, 1)?.earliest() {
        Ok(t.timestamp_millis())
    } else if let Some(t) = from_ymd_hms_ms(t, crate::tz_capnp::Tz::Utc, 0)?.earliest() {
        Ok(t.timestamp_millis())
    } else {
        Err(capnp::Error::failed(
            "Couldn't resolve timestamp even in UTC?".into(),
        ))
    }
}

fn to_datetime(tz: &Tz, timestamp: i64) -> Result<DateTime<Tz>> {
    if let Some(t) = tz.timestamp_millis_opt(timestamp).earliest() {
        Ok(t)
    } else if let Some(t) = tz.timestamp_millis_opt(timestamp + 3600001).earliest() {
        Ok(t)
    } else if let Some(t) = Tz::UTC.timestamp_millis_opt(timestamp).earliest() {
        Ok(t)
    } else {
        Err(eyre::eyre!("Invalid datetime"))
    }
}

impl Scheduler {
    #[allow(clippy::too_many_arguments)]
    pub async fn repeat(
        db: &SqliteDatabase,
        timestamp: i64,
        every_months: u32,
        every_days: u32,
        every_hours: i32,
        every_millis: i64,
        tz_index: u16,
        repeater: i64,
    ) -> Result<DateTime<Tz>> {
        let tz = TZ_MAP[tz_index as usize];

        // Build a datetime from our timezone and then use chrono's DST handling to add days and months.
        let t = to_datetime(&tz, timestamp)?;

        if repeater != 0 {
            let sturdyref = db.get_sturdyref(repeater)?;

            let callback: crate::scheduler_capnp::repeat_callback::Client =
                capnp::capability::FromClientHook::new(sturdyref.pipeline.get_cap().as_cap());

            let mut req = callback.repeat_request();
            let mut time = req.get().init_time();
            time.set_year(t.year() as i64);
            time.set_month(t.month() as u8);
            time.set_day(t.day() as u16);
            time.set_hour(t.hour() as u16);
            time.set_min(t.minute() as u16);
            time.set_sec(t.second() as i64);
            time.set_milli(t.timestamp_subsec_millis() as i64);
            time.set_tz(tz_index.try_into()?);

            to_datetime(
                &tz,
                get_timestamp(req.send().promise.await?.get()?.get_next()?)?,
            )
        } else {
            Ok(t + chrono::Days::new(every_days as u64)
                + chrono::Months::new(every_months)
                + chrono::TimeDelta::hours(every_hours as i64)
                + chrono::TimeDelta::milliseconds(every_millis))
        }
    }

    pub async fn run(
        db: Rc<SqliteDatabase>,
        action: i64,
    ) -> Result<crate::scheduler_capnp::action::Client> {
        let sturdyref = db.get_sturdyref(action)?;

        let action: crate::scheduler_capnp::action::Client =
            capnp::capability::FromClientHook::new(sturdyref.pipeline.get_cap().as_cap());

        // The RPC system will send this immediately, regardless of whether we await the future
        action.run_request().send().promise.await?;

        Ok(action)
    }

    async fn run_and_save(
        db: &Rc<SqliteDatabase>,
        id: i64,
        action: i64,
        update: &mut CachedStatement<'_>,
    ) -> Result<()> {
        let client = Self::run(db.clone(), action).await?;
        let action = db_save_action(db.clone(), client.client.hook).await?;
        update.execute(params![id, action])?;
        Ok(())
    }

    async fn catch_error<T>(e: eyre::ErrReport) -> capnp::Result<T> {
        eprintln!("Error when executing action: {e}");
        let r = capnp::Error::failed(e.to_string());
        LAST_ERROR.store(Some(Box::new(e)), std::sync::atomic::Ordering::AcqRel);
        Err(r)
    }

    async fn catchup(db: Rc<SqliteDatabase>, miss: MissBehavior, id: i64, count: i64, action: i64) {
        if miss != MissBehavior::None {
            let mut update = match db
                .connection
                .prepare_cached("UPDATE scheduler SET action = ?2 WHERE id = ?1")
            {
                Ok(a) => a,
                Err(e) => {
                    let _ = Self::catch_error::<()>(e.into()).await;
                    return;
                }
            };

            match miss {
                MissBehavior::One => {
                    let _ = Self::run_and_save(&db, id, action, &mut update)
                        .or_else(Self::catch_error)
                        .await;
                }
                MissBehavior::All => {
                    for _ in 0..count {
                        let _ = Self::run_and_save(&db, id, action, &mut update)
                            .or_else(Self::catch_error)
                            .await;
                    }
                }
                MissBehavior::None => (),
            }
        }
    }

    fn overflow_error(id: i64, remove: &mut CachedStatement<'_>) -> rusqlite::Result<usize> {
        eprintln!("Overflow error while processing {id}, canceling task");
        remove.execute(params![id])
    }

    #[allow(private_bounds)]
    pub fn new<TS: TimeSource + 'static>(
        db: Rc<SqliteDatabase>,
        mut ts: TS,
    ) -> Result<(Self, impl Future<Output = Result<()>>)> {
        // Prep database
        db.connection.execute(
            "
CREATE TABLE IF NOT EXISTS scheduler (
  id           INTEGER PRIMARY KEY,
  timestamp    INTEGER NOT NULL,
  action       BLOB NOT NULL,
  every_months INTEGER NOT NULL,
  every_days   INTEGER NOT NULL,
  every_hours  INTEGER NOT NULL,
  every_millis INTEGER NOT NULL,
  tz       INTEGER NOT NULL,
  repeat       INTEGER NOT NULL,
  miss       INTEGER NOT NULL
)",
            (),
        )?;

        let db2 = db.clone();
        // We allow buffering messages because we can deal with all the pending database changes in one query.
        let (send, mut recv) = mpsc::channel(20);

        // This absolutely must be in a spawn() call of some kind because a multithreaded runtime must be able to give it as much wakeup granularity as possible.
        let fut = async move {
            let mut get_range = db.connection.prepare_cached(
                "SELECT id, timestamp, action, every_months, every_days, every_hours, every_millis, tz, repeat FROM scheduler WHERE timestamp <= ?1",
            )?;
            let mut remove = db
                .connection
                .prepare_cached("DELETE FROM scheduler WHERE id = ?1")?;
            let mut set = db
                .connection
                .prepare_cached("UPDATE scheduler SET timestamp = ?2 WHERE id = ?1")?;
            let mut get_time = db
                .connection
                .prepare_cached("SELECT timestamp FROM scheduler ORDER BY timestamp ASC LIMIT 1")?;

            // Before we start looping, get backlog of missed tasks and run any that request to be run even if missed
            {
                let now = ts.now().await;

                let mut backlog = get_range.query(params![now])?;

                while let Some(row) = backlog.next()? {
                    let id: i64 = row.get(0)?;
                    let timestamp: i64 = row.get(1)?;
                    let action: i64 = row.get(2)?;
                    let every_months: i64 = row.get(3)?;
                    let every_days: i64 = row.get(4)?;
                    let every_hours: i64 = row.get(5)?;
                    let every_millis: i64 = row.get(6)?;
                    let tz_index: i64 = row.get(7)?;
                    let repeater: i64 = row.get(8)?;
                    let miss: MissBehavior = row.get::<usize, u16>(9)?.try_into()?;

                    // We can do a small trick here. We only have to repeat manually if it involves days or months, or a custom function.
                    if every_months != 0 || every_days != 0 || repeater != 0 {
                        let mut next = timestamp;
                        let mut count = 0;

                        while next <= now {
                            count += 1;
                            next = Self::repeat(
                                db.as_ref(),
                                next,
                                every_months as u32,
                                every_days as u32,
                                every_hours as i32,
                                every_millis,
                                tz_index as u16,
                                repeater,
                            )
                            .await?
                            .timestamp_millis();
                        }

                        tokio::task::spawn_local(Self::catchup(
                            db.clone(),
                            miss,
                            id,
                            count,
                            action,
                        ));

                        set.execute(params![id, next])?;
                    } else if every_hours != 0 || every_millis != 0 {
                        // Otherwise, this interval isn't affected by DST and we can calculate it directly
                        let Some(millis) = every_hours.checked_mul(3600000) else {
                            Self::overflow_error(id, &mut remove)?;
                            continue;
                        };
                        let Some(skip) = millis.checked_add(every_millis) else {
                            Self::overflow_error(id, &mut remove)?;
                            continue;
                        };
                        let Some(diff) = now.checked_sub(timestamp) else {
                            Self::overflow_error(id, &mut remove)?;
                            continue;
                        };
                        let count = (skip / diff) + 1;

                        tokio::task::spawn_local(Self::catchup(
                            db.clone(),
                            miss,
                            id,
                            count,
                            action,
                        ));

                        let Some(new_timestamp) = timestamp.checked_add(skip * count) else {
                            Self::overflow_error(id, &mut remove)?;
                            continue;
                        };

                        set.execute(params![id, new_timestamp])?;
                    } else if miss != MissBehavior::None {
                        // If there's no repeat information at all, this is a oneshot, so no need to re-save it.
                        tokio::task::spawn_local(
                            Self::run(db.clone(), action).or_else(Self::catch_error),
                        );
                        remove.execute(params![id])?;
                    };
                }
            }

            // Now we start our infinite loop where we sleep until we have something to execute
            loop {
                while recv.try_recv().is_ok() {} // drain messages before doing the database query
                let next: Option<i64> = get_time.query_row((), |row| row.get(0)).optional()?;
                // If there is no next event, we go to sleep forever, only awakening if we receive a database change signal.
                let now = ts.now().await;
                let diff = if let Some(ms) = next {
                    ms.checked_sub(now)
                        .expect("Overflow subtracting ms from now! Aborting!")
                } else {
                    86400 * 365 * 30 * 1000
                };

                // We only sleep if there isn't an event to immediately run.
                if diff > TIMER_EPSILON {
                    tokio::select! {
                      biased;

                        q = recv.recv() => {
                            if let Some(Queue::Cancel(id)) = q {
                                remove.execute(params![id])?;
                            }
                            // Check to see if we got woken up close enough to our target time (if it exists) to just treat this as a timer wakeup
                            if let Some(ms) = next {
                                let rediff = ms.checked_sub(ts.now().await);
                                if let Some(n) = rediff {
                                    if n >= TIMER_EPSILON {
                                        // If we weren't close enough, restart the loop to requery the database
                                        continue;
                                    }
                                } else {
                                    // If there's an overflow, definitely requery the database
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }
                        _ = ts.sleep(Instant::now() + Duration::from_millis(diff as u64)) => ()
                    };
                    //
                }

                let now = ts.now().await;

                let mut rows = get_range.query(params![now + TIMER_EPSILON])?;
                while let Some(row) = rows.next()? {
                    let id: i64 = row.get(0)?;
                    let timestamp: i64 = row.get(1)?;
                    let action: i64 = row.get(2)?;
                    let every_months: i64 = row.get(3)?;
                    let every_days: i64 = row.get(4)?;
                    let every_hours: i64 = row.get(5)?;
                    let every_millis: i64 = row.get(6)?;
                    let tz_index: i64 = row.get(7)?;
                    let repeater: i64 = row.get(8)?;

                    if every_months != 0
                        || every_days != 0
                        || repeater != 0
                        || every_hours != 0
                        || every_millis != 0
                    {
                        let new_timestamp = Self::repeat(
                            &db,
                            timestamp,
                            every_months as u32,
                            every_days as u32,
                            every_hours as i32,
                            every_millis,
                            tz_index as u16,
                            repeater,
                        )
                        .await?;

                        tokio::task::spawn_local(Self::catchup(
                            db.clone(),
                            MissBehavior::One,
                            id,
                            1,
                            action,
                        ));
                        set.execute(params![id, new_timestamp.timestamp_millis()])?;
                    } else {
                        tokio::task::spawn_local(
                            Self::run(db.clone(), action).or_else(Self::catch_error),
                        );
                        remove.execute(params![id])?;
                    }
                }
            }
        };

        Ok((
            Self {
                db: db2,
                signal: send,
            },
            fut,
        ))
    }
}

struct Registration {
    id: i64,
    signal: tokio::sync::mpsc::Sender<Queue>,
}

use crate::scheduler_capnp::registration;
use crate::scheduler_capnp::root;

#[capnproto_rpc(registration)]
impl registration::Server for Registration {
    async fn cancel(self: Rc<Self>) {
        self.signal.send(Queue::Cancel(self.id)).await.to_capnp()
    }
}

async fn db_save_action(db: Rc<SqliteDatabase>, hook: Box<dyn ClientHook>) -> capnp::Result<i64> {
    let saveable: crate::storage_capnp::saveable::Client<any_pointer> =
        capnp::capability::FromClientHook::new(hook);
    let response = saveable.save_request().send().promise.await?;
    let sturdyref = capnp::capability::get_resolved_cap(response.get()?.get_ref()?).await;
    db.get_sturdyref_id(sturdyref)
}

impl root::Server for Scheduler {
    async fn once_(
        self: Rc<Self>,
        params: root::OnceParams,
        mut results: root::OnceResults,
    ) -> Result<(), capnp::Error> {
        let params = params.get()?;
        let action_id = db_save_action(self.db.clone(), params.get_act()?.client.hook).await?;
        let miss = if params.get_fire_if_missed() {
            MissBehavior::One
        } else {
            MissBehavior::None
        };

        let mut add_once = self.db.connection.prepare_cached(
            "INSERT INTO scheduler (timestamp, action, every_months, every_days, every_hours, every_millis, tz, repeat, miss) VALUES(?1, ?2, 0, 0, 0, 0, 0, 0, ?3)",
        ).to_capnp()?;

        add_once
            .execute(params![
                get_timestamp(params.get_time()?)?,
                action_id,
                miss as i32
            ])
            .to_capnp()?;

        self.signal.send(Queue::Add).await.to_capnp()?;

        let reg = Registration {
            id: action_id,
            signal: self.signal.clone(),
        };
        results.get().set_res(capnp_rpc::new_client(reg));

        Ok(())
    }

    async fn every(
        self: Rc<Self>,
        params: root::EveryParams,
        mut results: root::EveryResults,
    ) -> Result<(), capnp::Error> {
        let params = params.get()?;

        let action_id = db_save_action(self.db.clone(), params.get_act()?.client.hook).await?;
        let miss = params.get_missed()?;
        let repeat = params.get_repeat()?;

        let mut add_every = self.db.connection.prepare_cached(
            "INSERT INTO scheduler (timestamp, action, every_months, every_days, every_hours, every_millis, tz, repeat, miss) VALUES(?1, ?2, ?4, ?5, ?6, ?7, ?8, 0, ?3)",
        ).to_capnp()?;

        add_every
            .execute(params![
                get_timestamp(params.get_time()?)?,
                action_id,
                miss as i32,
                repeat.get_months(),
                repeat.get_days(),
                repeat.get_hours(),
                repeat.get_millis(),
                params.get_time()?.get_tz()? as i64,
            ])
            .to_capnp()?;

        self.signal.send(Queue::Add).await.to_capnp()?;

        let reg = Registration {
            id: action_id,
            signal: self.signal.clone(),
        };
        results.get().set_res(capnp_rpc::new_client(reg));

        Ok(())
    }

    async fn complex(
        self: Rc<Self>,
        params: root::ComplexParams,
        mut results: root::ComplexResults,
    ) -> Result<(), capnp::Error> {
        let params = params.get()?;

        let action_id = db_save_action(self.db.clone(), params.get_act()?.client.hook).await?;
        let miss = params.get_missed()?;

        let repeat_id = db_save_action(self.db.clone(), params.get_repeat()?.client.hook).await?;

        let mut add_complex = self.db.connection.prepare_cached(
            "INSERT INTO scheduler (timestamp, action, every_months, every_days, every_hours, every_millis, tz, repeat, miss) VALUES(?1, ?2, 0, 0, 0, 0, 0, ?4, ?3)",
        ).to_capnp()?;

        add_complex
            .execute(params![
                get_timestamp(params.get_time()?)?,
                action_id,
                miss as i32,
                repeat_id,
            ])
            .to_capnp()?;

        self.signal.send(Queue::Add).await.to_capnp()?;

        let reg = Registration {
            id: action_id,
            signal: self.signal.clone(),
        };
        results.get().set_res(capnp_rpc::new_client(reg));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::capnp;
    use crate::capnp::any_pointer::Owned as any_pointer;
    use crate::capnp::capability::FromClientHook;
    use crate::capnp::capability::FromServer;
    use crate::capnp::private::capability::ClientHook;
    use crate::capnp_rpc;
    use crate::keystone::CapnpResult;
    use crate::scheduler_capnp::MissBehavior;
    use crate::scheduler_capnp::action;
    use crate::scheduler_capnp::repeat_callback;
    use crate::sqlite::SqliteDatabase;
    use crate::storage_capnp::restore;
    use crate::storage_capnp::saveable;
    use atomic_take::AtomicTake;
    use capnp_macros::capnproto_rpc;
    use chrono::Datelike;
    use chrono::TimeZone;
    use chrono::Timelike;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::atomic::AtomicI32;
    use std::sync::atomic::Ordering;
    use tokio::sync::mpsc;
    use tokio::sync::oneshot;
    use tokio::time::Instant;

    struct TestActionImpl {
        send: AtomicTake<oneshot::Sender<()>>,
        count: AtomicI32,
        key: String,
        parent: Rc<TestModule>,
    }

    #[capnproto_rpc(action)]
    impl action::Server for TestActionImpl {
        async fn run(self: Rc<Self>) -> capnp::Result<()> {
            let prev = self.count.fetch_sub(1, Ordering::AcqRel);
            tracing::info!("{}::run({})", self.key, prev);

            if prev == 1 {
                let test = self
                    .send
                    .take()
                    .ok_or(capnp::Error::failed("Race condition with sender?".into()))?;
                test.send(())
                    .map_err(|_| capnp::Error::failed("Failed to send signal".into()))?;
            } else if prev < 1 {
                tracing::error!("ERROR!!!");
                self.send.take().ok_or(capnp::Error::failed(
                    "Action called more than expected!".into(),
                ))?;
                tracing::error!("MORE ERROR!!!");
                return Err(capnp::Error::failed(
                    "Count was below 0, but we never called the action???".into(),
                ));
            }

            Ok(())
        }
    }

    #[capnproto_rpc(saveable)]
    impl saveable::Server<action::Owned> for TestActionImpl {
        async fn save(self: Rc<Self>) -> capnp::Result<()> {
            tracing::info!("{}::save() {:?}", self.key, &self.send);
            let sturdyref =
                crate::sturdyref::SturdyRefImpl::init(1, self.key.as_str(), self.parent.db.clone())
                    .await
                    .to_capnp()?;

            let cap: crate::storage_capnp::sturdy_ref::Client<any_pointer> = self
                .parent
                .db
                .sturdyref_set
                .borrow_mut()
                .new_client(sturdyref);

            let cap: crate::storage_capnp::sturdy_ref::Client<action::Owned> = cap.client.cast_to();
            results.get().set_ref(cap);

            // After our final expected call of run(), send will be empty, and us calling save() on it is not a bug, so we can't detect it here.
            if let Some(send) = self.send.take() {
                self.parent
                    .actions
                    .borrow_mut()
                    .insert(self.key.clone(), (self.count.load(Ordering::Acquire), send));
            }
            Ok(())
        }
    }

    struct TestTimeSource {
        now: mpsc::Receiver<i64>,
        waker: mpsc::Receiver<()>,
    }

    impl super::TimeSource for TestTimeSource {
        async fn now(&mut self) -> i64 {
            tracing::info!(concat!("NOW: ", file!(), ":", line!()));
            self.now
                .recv()
                .await
                .expect("Time channel closed unexpectedly")
        }

        async fn sleep(&mut self, _: Instant) {
            tracing::info!(concat!("SLEEP: ", file!(), ":", line!()));
            self.waker.recv().await.expect("waker closed unexpectedly!");
        }
    }

    struct TestModule {
        db: Rc<SqliteDatabase>,
        actions: RefCell<HashMap<String, (i32, oneshot::Sender<()>)>>,
    }

    struct TestRepeatCallback {
        db: Rc<SqliteDatabase>,
    }

    #[capnproto_rpc(repeat_callback)]
    impl repeat_callback::Server for TestRepeatCallback {
        async fn repeat(self: Rc<Self>, time: Reader) -> capnp::Result<()> {
            let tz_index = time.get_tz()?;
            let datetime = super::from_ymd_hms_ms(time, tz_index, 0)?
                .earliest()
                .unwrap();
            let t = datetime.checked_add_months(chrono::Months::new(1)).unwrap();

            let mut time = results.get().init_next();
            time.set_year(t.year() as i64);
            time.set_month(t.month() as u8);
            time.set_day(t.day() as u16);
            time.set_hour(t.hour() as u16);
            time.set_min(t.minute() as u16);
            time.set_sec(t.second() as i64);
            time.set_milli(t.timestamp_subsec_millis() as i64);
            time.set_tz(tz_index.try_into().unwrap());

            Ok(())
        }
    }

    #[capnproto_rpc(saveable)]
    impl saveable::Server<repeat_callback::Owned> for TestRepeatCallback {
        async fn save(self: Rc<Self>) -> capnp::Result<()> {
            let sturdyref = crate::sturdyref::SturdyRefImpl::init(1, "__callback", self.db.clone())
                .await
                .to_capnp()?;

            let cap: crate::storage_capnp::sturdy_ref::Client<any_pointer> =
                self.db.sturdyref_set.borrow_mut().new_client(sturdyref);

            let cap: crate::storage_capnp::sturdy_ref::Client<repeat_callback::Owned> =
                cap.client.cast_to();
            results.get().set_ref(cap);
            Ok(())
        }
    }

    impl restore::Server<any_pointer> for TestModule {
        async fn restore(
            self: Rc<Self>,
            params: restore::RestoreParams<any_pointer>,
            mut results: restore::RestoreResults<any_pointer>,
        ) -> Result<(), capnp::Error> {
            let key: capnp::text::Reader = params.get()?.get_data()?.get_as()?;
            let key = key.to_string()?;

            if key == "__callback" {
                let client: repeat_callback::Client = capnp_rpc::new_client(TestRepeatCallback {
                    db: self.db.clone(),
                });

                results
                    .get()
                    .init_cap()
                    .set_as_capability(client.client.hook);
            } else {
                let (count, send) = self
                    .actions
                    .borrow_mut()
                    .remove(&key)
                    .ok_or(capnp::Error::failed(
                        "Tried to restore nonexistent oneshot sender!".into(),
                    ))?
                    .into();

                let client: action::Client = capnp_rpc::new_client(TestActionImpl {
                    send: send.into(),
                    key,
                    count: count.into(),
                    parent: self.clone(),
                });

                results
                    .get()
                    .init_cap()
                    .set_as_capability(client.client.hook);
            }

            Ok(())
        }
    }

    fn build_test_scheduler(
        db_path: &tempfile::TempPath,
    ) -> eyre::Result<(
        crate::scheduler_capnp::root::Client,
        mpsc::Sender<()>,
        mpsc::Sender<i64>,
        Rc<TestModule>,
    )> {
        let db = crate::database::open_database(
            db_path.to_path_buf(),
            |c| SqliteDatabase::new_connection(c).map(|s| Rc::new(s)),
            crate::database::OpenOptions::Create,
        )?;

        let module: Rc<TestModule> = Rc::new(TestModule {
            db: db.clone(),
            actions: RefCell::new(HashMap::new()),
        });

        db.clients.borrow_mut().insert(
            1,
            capnp_rpc::local::Client::new(restore::Client::from_rc(module.clone())).add_ref(),
        );
        let (sleep, waker) = mpsc::channel(1);
        let (time, now) = mpsc::channel(1);
        let (s, fut) = super::Scheduler::new(db, TestTimeSource { now, waker })?;

        let scheduler =
            capnp_rpc::local::Client::new(crate::scheduler_capnp::root::Client::from_server(s));

        let hook = scheduler.add_ref();

        tokio::task::spawn_local(fut);
        Ok((FromClientHook::new(hook), sleep, time, module))
    }

    async fn build_test_action(
        name: impl AsRef<str>,
        datetime: chrono::DateTime<chrono::Utc>,
        delay: i64,
        scheduler: &crate::scheduler_capnp::root::Client,
        module: Rc<TestModule>,
    ) -> capnp::Result<(
        oneshot::Receiver<()>,
        crate::scheduler_capnp::registration::Client,
    )> {
        let (sender, finished) = oneshot::channel();
        let action = TestActionImpl {
            send: sender.into(),
            key: name.as_ref().into(),
            count: 1.into(),
            parent: module,
        };

        let cancel = scheduler
            .build_once_request(
                Some(crate::scheduler_capnp::calendar_time::CalendarTime {
                    _year: datetime.year() as i64,
                    _month: datetime.month() as u8,
                    _day: datetime.day() as u16,
                    _hour: datetime.hour() as u16,
                    _min: datetime.minute() as u16,
                    _sec: datetime.second() as i64,
                    _milli: datetime.timestamp_subsec_millis() as i64 + delay,
                    _tz: crate::tz_capnp::Tz::Utc,
                }),
                capnp_rpc::new_client(action),
                true,
            )
            .send()
            .promise
            .await?;

        Ok((finished, cancel.get()?.get_res()?))
    }

    async fn build_test_repeat(
        name: impl AsRef<str>,
        datetime: chrono::DateTime<chrono::Utc>,
        repeat: i32,
        delay: i64,
        months: i64,
        days: i64,
        millis: i64,
        miss: MissBehavior,
        scheduler: &crate::scheduler_capnp::root::Client,
        module: Rc<TestModule>,
    ) -> capnp::Result<(
        oneshot::Receiver<()>,
        crate::scheduler_capnp::registration::Client,
    )> {
        let (sender, finished) = oneshot::channel();
        let action = TestActionImpl {
            send: sender.into(),
            key: name.as_ref().into(),
            count: repeat.into(),
            parent: module,
        };

        let cancel = scheduler
            .build_every_request(
                Some(crate::scheduler_capnp::calendar_time::CalendarTime {
                    _year: datetime.year() as i64,
                    _month: datetime.month() as u8,
                    _day: datetime.day() as u16,
                    _hour: datetime.hour() as u16,
                    _min: datetime.minute() as u16,
                    _sec: datetime.second() as i64,
                    _milli: datetime.timestamp_subsec_millis() as i64 + delay,
                    _tz: crate::tz_capnp::Tz::Utc,
                }),
                Some(crate::scheduler_capnp::every::Every {
                    _months: months as u32,
                    _days: days as u64,
                    _hours: 0,
                    _millis: millis,
                }),
                capnp_rpc::new_client(action),
                miss,
            )
            .send()
            .promise
            .await?;

        Ok((finished, cancel.get()?.get_res()?))
    }

    async fn build_test_callback(
        name: impl AsRef<str>,
        datetime: chrono::DateTime<chrono::Utc>,
        repeat: i32,
        delay: i64,
        callback: repeat_callback::Client,
        miss: MissBehavior,
        scheduler: &crate::scheduler_capnp::root::Client,
        module: Rc<TestModule>,
    ) -> capnp::Result<(
        oneshot::Receiver<()>,
        crate::scheduler_capnp::registration::Client,
    )> {
        let (sender, finished) = oneshot::channel();
        let action = TestActionImpl {
            send: sender.into(),
            key: name.as_ref().into(),
            count: repeat.into(),
            parent: module,
        };

        let cancel = scheduler
            .build_complex_request(
                Some(crate::scheduler_capnp::calendar_time::CalendarTime {
                    _year: datetime.year() as i64,
                    _month: datetime.month() as u8,
                    _day: datetime.day() as u16,
                    _hour: datetime.hour() as u16,
                    _min: datetime.minute() as u16,
                    _sec: datetime.second() as i64,
                    _milli: datetime.timestamp_subsec_millis() as i64 + delay,
                    _tz: crate::tz_capnp::Tz::Utc,
                }),
                callback,
                capnp_rpc::new_client(action),
                miss,
            )
            .send()
            .promise
            .await?;

        Ok((finished, cancel.get()?.get_res()?))
    }

    #[tokio::test]
    async fn test_basic_scheduler() -> eyre::Result<()> {
        let pool = tokio::task::LocalSet::new();
        pool.run_until(async move {
            let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            let (scheduler, wake, time, module) = build_test_scheduler(&db_path)?;

            let datetime = chrono::Utc.with_ymd_and_hms(2023, 12, 31, 1, 2, 3).unwrap();
            let timestamp = datetime.timestamp_millis();
            let (mut finished, _) =
                build_test_action("basic", datetime, 1, &scheduler, module.clone()).await?;

            time.send(timestamp - 100).await?;
            time.send(timestamp - 100).await?;
            wake.send(()).await?;
            time.send(timestamp - 100).await?;
            wake.send(()).await?;
            assert_eq!(
                finished.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            );
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            assert_eq!(finished.await, Ok(()));

            Ok::<(), eyre::Report>(())
        })
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_schedule_cancel() -> eyre::Result<()> {
        let pool = tokio::task::LocalSet::new();
        pool.run_until(async move {
            let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            let (scheduler, wake, time, module) = build_test_scheduler(&db_path)?;

            let datetime = chrono::Utc.with_ymd_and_hms(2023, 12, 31, 1, 2, 3).unwrap();
            let timestamp = datetime.timestamp_millis();
            let (mut finished, cancel) =
                build_test_action("cancel", datetime, 100, &scheduler, module.clone()).await?;

            time.send(timestamp).await?;
            time.send(timestamp).await?;
            wake.send(()).await?;
            cancel.cancel_request().send().promise.await?;
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            wake.send(()).await?;
            assert_eq!(
                finished.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            );

            Ok::<(), eyre::Report>(())
        })
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_scheduler_repeat() -> eyre::Result<()> {
        let pool = tokio::task::LocalSet::new();
        pool.run_until(async move {
            let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            let (scheduler, wake, time, module) = build_test_scheduler(&db_path)?;

            let datetime = chrono::Utc.with_ymd_and_hms(2023, 12, 31, 1, 2, 3).unwrap();
            let timestamp = datetime.timestamp_millis();
            let (mut finished, _) = build_test_repeat(
                "basic",
                datetime,
                3,
                1,
                0,
                0,
                1000,
                MissBehavior::None,
                &scheduler,
                module.clone(),
            )
            .await?;

            time.send(timestamp).await?;
            wake.send(()).await?;
            assert_eq!(
                finished.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            );
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            wake.send(()).await?;
            time.send(timestamp + 1000).await?;
            time.send(timestamp + 1000).await?;
            wake.send(()).await?;
            time.send(timestamp + 2000).await?;
            time.send(timestamp + 2000).await?;
            assert_eq!(finished.await, Ok(()));

            Ok::<(), eyre::Report>(())
        })
        .await?;

        if let Some(e) = super::LAST_ERROR.take(Ordering::AcqRel) {
            Err(*e)
        } else {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_scheduler_repeat_months() -> eyre::Result<()> {
        let pool = tokio::task::LocalSet::new();
        pool.run_until(async move {
            let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            let (scheduler, wake, time, module) = build_test_scheduler(&db_path)?;

            let datetime = chrono::Utc.with_ymd_and_hms(2023, 12, 31, 1, 2, 3).unwrap();
            let timestamp = datetime.timestamp_millis();
            let (mut finished, _) = build_test_repeat(
                "basic",
                datetime,
                3,
                1,
                1,
                0,
                0,
                MissBehavior::None,
                &scheduler,
                module.clone(),
            )
            .await?;

            time.send(timestamp).await?;
            wake.send(()).await?;
            assert_eq!(
                finished.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            );
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            wake.send(()).await?;
            let timestamp = datetime
                .checked_add_months(chrono::Months::new(1))
                .unwrap()
                .timestamp_millis();
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            wake.send(()).await?;
            let timestamp = datetime
                .checked_add_months(chrono::Months::new(2))
                .unwrap()
                .timestamp_millis();
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            assert_eq!(finished.await, Ok(()));

            Ok::<(), eyre::Report>(())
        })
        .await?;

        if let Some(e) = super::LAST_ERROR.take(Ordering::AcqRel) {
            Err(*e)
        } else {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_scheduler_repeat_cancel() -> eyre::Result<()> {
        let pool = tokio::task::LocalSet::new();
        pool.run_until(async move {
            let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            let (scheduler, wake, time, module) = build_test_scheduler(&db_path)?;

            let datetime = chrono::Utc.with_ymd_and_hms(2023, 12, 31, 1, 2, 3).unwrap();
            let timestamp = datetime.timestamp_millis();
            let (mut finished, cancel) = build_test_repeat(
                "basic",
                datetime,
                2,
                1,
                1,
                0,
                0,
                MissBehavior::None,
                &scheduler,
                module.clone(),
            )
            .await?;

            time.send(timestamp).await?;
            wake.send(()).await?;
            assert_eq!(
                finished.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            );
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            wake.send(()).await?;
            let timestamp = datetime
                .checked_add_months(chrono::Months::new(1))
                .unwrap()
                .timestamp_millis();
            time.send(timestamp).await?;
            time.send(timestamp).await?;

            cancel.cancel_request().send().promise.await?;
            let timestamp = datetime
                .checked_add_months(chrono::Months::new(2))
                .unwrap()
                .timestamp_millis();
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            assert_eq!(finished.await, Ok(()));

            Ok::<(), eyre::Report>(())
        })
        .await?;

        if let Some(e) = super::LAST_ERROR.take(Ordering::AcqRel) {
            Err(*e)
        } else {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_scheduler_repeat_callback() -> eyre::Result<()> {
        let pool = tokio::task::LocalSet::new();
        pool.run_until(async move {
            let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            let (scheduler, wake, time, module) = build_test_scheduler(&db_path)?;

            let datetime = chrono::Utc.with_ymd_and_hms(2023, 12, 31, 1, 2, 3).unwrap();
            let timestamp = datetime.timestamp_millis();

            let callback: repeat_callback::Client = capnp_rpc::new_client(TestRepeatCallback {
                db: module.db.clone(),
            });
            let (mut finished, _) = build_test_callback(
                "basic",
                datetime,
                3,
                1,
                callback,
                MissBehavior::None,
                &scheduler,
                module.clone(),
            )
            .await?;

            time.send(timestamp).await?;
            wake.send(()).await?;
            assert_eq!(
                finished.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            );
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            wake.send(()).await?;
            let timestamp = datetime
                .checked_add_months(chrono::Months::new(1))
                .unwrap()
                .timestamp_millis();
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            wake.send(()).await?;
            let timestamp = datetime
                .checked_add_months(chrono::Months::new(2))
                .unwrap()
                .timestamp_millis();
            time.send(timestamp).await?;
            time.send(timestamp).await?;
            assert_eq!(finished.await, Ok(()));

            Ok::<(), eyre::Report>(())
        })
        .await?;

        if let Some(e) = super::LAST_ERROR.take(Ordering::AcqRel) {
            Err(*e)
        } else {
            Ok(())
        }
    }
}
