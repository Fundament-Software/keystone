use capnp::introspect::Introspect;
use capnp_macros::capnproto_rpc;
use chrono::DateTime;
use chrono::Datelike;
use chrono::TimeZone;
use chrono::Timelike;
use rusqlite::OptionalExtension;

use crate::database::DatabaseExt;
use crate::keystone::CapnpResult;
use crate::sqlite::SqliteDatabase;
use crate::sqlite_capnp::root::ServerDispatch;
use chrono_tz::Tz;
use eyre::Result;
use futures_util::TryFutureExt;
use rusqlite::params;
use std::rc::Rc;
use std::str::FromStr;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
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
const TZ_MAP: LazyLock<[Tz; 596]> = LazyLock::new(|| gen_tz_map());

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
    db: Rc<ServerDispatch<SqliteDatabase>>,
    pub thread: JoinHandle<Result<()>>,
    signal: tokio::sync::mpsc::Sender<Queue>,
}

const TIMER_EPSILON: i64 = 20; // ms

#[inline]
fn from_ymh_hms_ms(
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
    if let Some(t) = from_ymh_hms_ms(t, tz_index, 0)?.earliest() {
        Ok(t.timestamp_millis())
    } else if let Some(t) = from_ymh_hms_ms(t, tz_index, 1)?.earliest() {
        Ok(t.timestamp_millis())
    } else if let Some(t) = from_ymh_hms_ms(t, crate::tz_capnp::Tz::Utc, 0)?.earliest() {
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
    pub async fn repeat(
        db: &Rc<ServerDispatch<SqliteDatabase>>,
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

    pub fn run(db: &Rc<ServerDispatch<SqliteDatabase>>, action: i64) {
        let Ok(sturdyref) = db.get_sturdyref(action) else {
            eprintln!("Error retrieving sturdyref with ID {}", action);
            return;
        };

        let action: crate::scheduler_capnp::action::Client =
            capnp::capability::FromClientHook::new(sturdyref.pipeline.get_cap().as_cap());

        // The RPC system will send this immediately, regardless of whether we await the future, but we spawn something for it anyway so we can print an error message if it fails.
        let promise: capnp::capability::Promise<
            capnp::capability::Response<crate::scheduler_capnp::action::run_results::Owned>,
            capnp::Error,
        > = action.run_request().send().promise;
        tokio::task::spawn_local(promise.or_else(|e| async move {
            eprintln!("Error running action: {}", e.to_string());
            Err(e)
        }));
    }

    pub fn new<TS: TimeSource + 'static>(
        db: Rc<ServerDispatch<SqliteDatabase>>,
        mut ts: TS,
    ) -> Result<Self> {
        // Prep database
        db.server.connection.execute(
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

        let add_complex = db.connection.prepare_cached(
            "INSERT INTO scheduler (timestamp, action, every_months, every_days, every_hours, every_millis, tz, repeat, miss) VALUES(?1, ?2, 0, 0, 0, 0, 0, ?4, ?3)",
        )?;

        let db2 = db.clone();
        // We allow buffering messages because we can deal with all the pending database changes in one query.
        let (send, mut recv) = mpsc::channel(20);

        // This absolutely must be in a spawn() call of some kind because a multithreaded runtime must be able to give it as much wakeup granularity as possible.
        let handle = tokio::task::spawn_local(async move {
            let mut get_range = db2.server.connection.prepare_cached(
                "SELECT id, timestamp, action, every_months, every_days, every_hours, every_millis, tz, repeat FROM scheduler WHERE timestamp <= ?1",
            )?;
            let mut remove = db2
                .server
                .connection
                .prepare_cached("DELETE FROM scheduler WHERE id = ?1")?;
            let mut set = db2
                .server
                .connection
                .prepare_cached("UPDATE scheduler SET timestamp = ?2 WHERE id = ?1")?;
            let mut get_time = db2
                .server
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
                    let miss: crate::scheduler_capnp::MissBehavior =
                        row.get::<usize, u16>(9)?.try_into()?;

                    // We can do a small trick here. We only have to repeat manually if it involves days or months, or a custom function.
                    let repeat = if every_months != 0 || every_days != 0 || repeater != 0 {
                        let mut next = timestamp;

                        while next <= now {
                            next = Self::repeat(
                                &db2,
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

                            if miss == crate::scheduler_capnp::MissBehavior::All {
                                Self::run(&db2, action);
                            }
                        }

                        if miss == crate::scheduler_capnp::MissBehavior::One {
                            Self::run(&db2, action);
                        }

                        Some(next)
                    } else if every_hours != 0 || every_millis != 0 {
                        // Otherwise, this interval isn't affected by DST and we can calculate it directly
                        let skip = (every_hours * 3600000) + every_millis;
                        let diff = now - timestamp;
                        let count = (skip / diff) + 1;

                        match miss {
                            crate::scheduler_capnp::MissBehavior::One => {
                                Self::run(&db2, action);
                            }
                            crate::scheduler_capnp::MissBehavior::All => {
                                for _ in 0..count {
                                    Self::run(&db2, action);
                                }
                            }
                            crate::scheduler_capnp::MissBehavior::None => (),
                        }

                        Some(timestamp + (skip * count))
                    } else {
                        // If there's no repeat information at all, this is a oneshot
                        Self::run(&db2, action);
                        None
                    };

                    if let Some(new_timestamp) = repeat {
                        set.execute(params![id, new_timestamp])?;
                    } else {
                        remove.execute(params![id])?;
                    }
                }
            }

            loop {
                while let Ok(_) = recv.try_recv() {} // drain messages before doing the database query
                let next: Option<i64> = get_time.query_row((), |row| row.get(0)).optional()?;
                // If there is no next event, we go to sleep forever, only awakening if we receive a database change signal.
                let now = ts.now().await;
                let diff = if let Some(ms) = next {
                    ms - now
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
                                if !((ms - ts.now().await) < TIMER_EPSILON) {
                                    // If we weren't close enough, restart the loop to requery the database
                                    continue;
                                }
                            } else {
                                continue;
                            }
                            ()
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

                    Self::run(&db2, action);

                    if every_months != 0
                        || every_days != 0
                        || repeater != 0
                        || every_hours != 0
                        || every_millis != 0
                    {
                        let new_timestamp = Self::repeat(
                            &db2,
                            timestamp,
                            every_months as u32,
                            every_days as u32,
                            every_hours as i32,
                            every_millis,
                            tz_index as u16,
                            repeater,
                        )
                        .await?;

                        set.execute(params![id, new_timestamp.timestamp_millis()])?;
                    } else {
                        remove.execute(params![id])?;
                    }
                }
            }
        });

        Ok(Self {
            db: db.clone(),
            thread: handle,
            signal: send,
        })
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
    async fn cancel(&self) {
        self.signal.send(Queue::Cancel(self.id)).await.to_capnp()
    }
}

#[capnproto_rpc(root)]
impl root::Server for Scheduler {
    async fn once_(&self, time: Reader, act: Reader, fire_if_missed: bool) {
        let a: crate::scheduler_capnp::action::Client = act;
        let saveable: crate::storage_capnp::saveable::Client<capnp::any_pointer::Owned> =
            capnp::capability::FromClientHook::new(a.client.hook);

        let response = saveable.save_request().send().promise.await?;
        let sturdyref = capnp::capability::get_resolved_cap(response.get()?.get_ref()?).await;
        let action_id = self.db.get_sturdyref_id(sturdyref)?;
        let miss = if fire_if_missed {
            crate::scheduler_capnp::MissBehavior::One
        } else {
            crate::scheduler_capnp::MissBehavior::None
        };

        let mut add_once = self.db.connection.prepare_cached(
            "INSERT INTO scheduler (timestamp, action, every_months, every_days, every_hours, every_millis, tz, repeat, miss) VALUES(?1, ?2, 0, 0, 0, 0, 0, 0, ?3)",
        ).to_capnp()?;

        add_once
            .execute(params![get_timestamp(time)?, action_id, miss as i32])
            .to_capnp()?;

        self.signal.send(Queue::Add).await.to_capnp()?;

        let reg = Registration {
            id: action_id,
            signal: self.signal.clone(),
        };
        results.get().set_res(capnp_rpc::new_client(reg));

        Ok(())
    }

    /*async fn every(&self, time: Reader, repeat: Reader, act: Reader) {
        Result::<(), capnp::Error>::Err(::capnp::Error::unimplemented(
            "method root::Server::every not implemented".to_string(),
        ))

        let add_every = db.connection.prepare_cached(
            "INSERT INTO scheduler (timestamp, action, every_months, every_days, every_hours, every_millis, tz, repeat, miss) VALUES(?1, ?2, ?4, ?5, ?6, ?7, ?8, 0, ?3)",
        )?;
    }*/
}

#[cfg(test)]
mod tests {
    use crate::scheduler_capnp::action;
    use crate::storage_capnp::saveable;
    use atomic_take::AtomicTake;
    use capnp::capability::FromClientHook;
    use capnp::capability::FromServer;
    use capnp::private::capability::ClientHook;
    use capnp_macros::capnproto_rpc;
    use chrono::Datelike;
    use chrono::Timelike;
    use tokio::sync::mpsc;
    use tokio::sync::oneshot;
    use tokio::time::Instant;

    struct TestActionImpl {
        send: AtomicTake<oneshot::Sender<()>>,
    }

    #[capnproto_rpc(action)]
    impl action::Server for TestActionImpl {
        async fn run(&self) -> capnp::Result<()> {
            self.send
                .take()
                .expect("Tried to run action twice!")
                .send(())
                .unwrap();
            Ok(())
        }
    }

    #[capnproto_rpc(saveable)]
    impl saveable::Server<action::Owned> for TestActionImpl {
        async fn save(&self) -> capnp::Result<()> {
            panic!("aw fuck");
        }
    }

    struct TestTimeSource {
        now: mpsc::Receiver<i64>,
        waker: mpsc::Receiver<()>,
    }

    impl super::TimeSource for TestTimeSource {
        async fn now(&mut self) -> i64 {
            self.now
                .recv()
                .await
                .expect("Time channel closed unexpectedly")
        }

        async fn sleep(&mut self, deadline: Instant) {
            self.waker.recv().await.expect("waker closed unexpectedly!");
        }
    }

    fn build_test_scheduler(
        db_path: &tempfile::TempPath,
    ) -> eyre::Result<(
        crate::scheduler_capnp::root::Client,
        mpsc::Sender<()>,
        mpsc::Sender<i64>,
    )> {
        let db = crate::sqlite::SqliteDatabase::new(
            db_path.to_path_buf(),
            rusqlite::OpenFlags::default(),
            Default::default(),
            Default::default(),
        )?;

        let (sleep, waker) = mpsc::channel(1);
        let (time, now) = mpsc::channel(1);
        let scheduler =
            capnp_rpc::local::Client::new(crate::scheduler_capnp::root::Client::from_server(
                super::Scheduler::new(db, TestTimeSource { now, waker })?,
            ));

        let hook = scheduler.add_ref();

        Ok((FromClientHook::new(hook), sleep, time))
    }

    #[tokio::test]
    async fn test_basic_scheduler() -> eyre::Result<()> {
        let pool = tokio::task::LocalSet::new();
        pool.run_until(async move {
            let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
            let (scheduler, wake, time) = build_test_scheduler(&db_path)?;

            let (sender, mut finished) = oneshot::channel();
            let datetime = chrono::offset::Utc::now();
            let timestamp = datetime.timestamp_millis();
            let action = TestActionImpl {
                send: sender.into(),
            };

            scheduler.build_once_request(
                Some(crate::scheduler_capnp::calendar_time::CalendarTime {
                    _year: datetime.year() as i64,
                    _month: datetime.month() as u8,
                    _day: datetime.day() as u16,
                    _hour: datetime.hour() as u16,
                    _min: datetime.minute() as u16,
                    _sec: datetime.second() as i64,
                    _milli: datetime.timestamp_subsec_millis() as i64 + 1,
                    _tz: crate::tz_capnp::Tz::Utc,
                }),
                capnp_rpc::new_client(action),
                true,
            );

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
            assert_eq!(finished.blocking_recv(), Ok(()));
            Ok::<(), eyre::Report>(())
        })
        .await?;
        Ok(())
    }
}
