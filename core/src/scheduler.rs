use std::{collections::HashMap, time::{Duration, SystemTime, UNIX_EPOCH}, cell::RefCell, rc::Rc, sync::{Arc, RwLock}};

use capnp::{capability::{Promise, Request}, Error};
use capnp_macros::capnp_let;
//use capnp_macros::capnp_build;
use capnp_rpc::{pry, CapabilityServerSet};
use chrono::{DateTime, TimeZone};
use chrono_tz::Tz;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use tokio::{task::{JoinHandle, self, LocalSet}, time};
use capnp::IntoResult;
use keystone::sturdyref_capnp::saveable;
use tokio_util::time::{DelayQueue, delay_queue::Key};
use crate::{scheduler_capnp::{scheduler, cancelable, listener, create_scheduler, listener_test}, cap_std_capnproto, cap_std_capnp::ambient_authority::MonotonicClockNewResults};
use capnp::capability::FromClientHook;
use tokio_stream::StreamExt;

struct CreateSchedulerImpl {

}

impl create_scheduler::Server for CreateSchedulerImpl {
    fn create(&mut self, params: create_scheduler::CreateParams, mut result: create_scheduler::CreateResults) -> Promise<(), Error> {
        todo!()
    }
}
    
/*
struct SchedulerImpl {
    tasks: HashMap<u8, JoinHandle<()>>,
    next_id: u8,
    channel: sender: tokio::sync::mpsc::Sender<u8>,
    listeners: HashMap<u8, crate::scheduler_capnp::listener::Client>,
    sturdyrefs: HashMap<u8, Vec<u8>>
}

impl scheduler::Server for Rc<tokio::sync::RwLock<SchedulerImpl>> {
    fn repeat(&mut self, params: scheduler::RepeatParams, mut result: scheduler::RepeatResults) -> Promise<(), Error> {
    /*return tokio::task::block_in_place(move || {
    //Promise::from_future(async move {
        let mut write_guard = self.blocking_write();
        let params_reader = pry!(params.get());
        let listener = pry!(params_reader.get_listener());
        capnp_let!({delay : {secs, millis}} = params_reader);
        let dur = Duration::from_secs(secs) + Duration::from_millis(millis);
        
        let next_id = write_guard.next_id;
        let sender = write_guard.channel.sender.clone();
        write_guard.listeners.insert(next_id, listener);
        let handle = repeat_helper(dur, next_id, sender);
        write_guard.tasks.insert(next_id, handle);
        result.get().set_id(next_id);
        result.get().set_cancelable(capnp_rpc::new_client(CancelableImpl{id: next_id, rc: self.clone()}));
        write_guard.next_id += 1;
        drop(write_guard);
        return Promise::<(), Error>::ok(());
    })*/
        //Promise::ok(())
    let this = self.clone();
    let params_reader = pry!(params.get());
    let listener = pry!(params_reader.get_listener());
    capnp_let!({delay : {secs, millis}} = params_reader);
    return Promise::from_future(async move {
        let mut write_guard = this.write().await;
        let dur = Duration::from_secs(secs) + Duration::from_millis(millis);
        let next_id = write_guard.next_id;
        let sender = write_guard.channel.sender.clone();
        write_guard.listeners.insert(next_id, listener);
        let handle = repeat_helper(dur, next_id, sender);
        write_guard.tasks.insert(next_id, handle);
        result.get().set_id(next_id);
        result.get().set_cancelable(capnp_rpc::new_client(CancelableImpl{id: next_id, rc: this.clone()}));
        write_guard.next_id += 1;
        drop(write_guard);
        return Result::Ok(())
    });
    }
    fn schedule_for(&mut self, params: scheduler::ScheduleForParams, mut result: scheduler::ScheduleForResults) -> Promise<(), Error> {
        let this = self.clone();
        let params_reader = pry!(params.get());
        let listener = pry!(params_reader.get_listener());
        capnp_let!({schedule_for : {year, month, day, hour, min}} = params_reader);
        return Promise::from_future(async move {
            let mut write_guard = this.write().await;
            let Some(utc_date) = chrono::NaiveDate::from_ymd_opt(year, month, day) else {
                return Err(capnp::Error{kind: capnp::ErrorKind::Failed, extra: String::from("Date out of range")});
            };
            let Some(utc_time) = chrono::NaiveTime::from_hms_opt(hour, min, 0) else {
                return Err(capnp::Error{kind: capnp::ErrorKind::Failed, extra: String::from("Invalid hour or minute")});
            };
            let utc_date_time = chrono::NaiveDateTime::new(utc_date, utc_time).and_utc();
            let current = chrono::offset::Utc::now();
            let Ok(dur) = (utc_date_time - current).to_std() else {
                return Err(capnp::Error{kind: capnp::ErrorKind::Failed, extra: String::from("Scheduled date time before current")});
            };
            let next_id = write_guard.next_id;
            let sender_channel = write_guard.channel.sender.clone();
            write_guard.listeners.insert(next_id, listener);
            let handle = tokio::task::spawn (async move {
                let id = next_id;
                tokio::time::sleep(dur).await;
                let Ok(()) = sender_channel.send(id).await else {
                    return;
                };
            });
            write_guard.tasks.insert(next_id, handle);
            result.get().set_id(next_id);
            result.get().set_cancelable(capnp_rpc::new_client(CancelableImpl{id: next_id, rc: this.clone()}));
            write_guard.next_id += 1;
            drop(write_guard);
            return Result::Ok(())
        });
    }
    fn schedule_for_then_repeat(&mut self, params: scheduler::ScheduleForThenRepeatParams, mut result: scheduler::ScheduleForThenRepeatResults) -> Promise<(), Error> {
        let this = self.clone();
        let params_reader = pry!(params.get());
        let listener = pry!(params_reader.get_listener());
        capnp_let!({start : {year, month, day, hour, min}} = params_reader);
        capnp_let!({delay : {secs, millis}} = params_reader);
        return Promise::from_future(async move {
            let mut write_guard = this.write().await;
            let Some(utc_date) = chrono::NaiveDate::from_ymd_opt(year, month, day) else {
                return Err(capnp::Error{kind: capnp::ErrorKind::Failed, extra: String::from("Date out of range")});
            };
            let Some(utc_time) = chrono::NaiveTime::from_hms_opt(hour, min, 0) else {
                return Err(capnp::Error{kind: capnp::ErrorKind::Failed, extra: String::from("Invalid hour or minute")});
            };
            let utc_date_time = chrono::NaiveDateTime::new(utc_date, utc_time).and_utc();
            let current = chrono::offset::Utc::now();
            let Ok(duration_until_start) = (utc_date_time - current).to_std() else {
                return Err(capnp::Error{kind: capnp::ErrorKind::Failed, extra: String::from("Scheduled date time before current")});
            };
            let duration_between_repeats = Duration::from_secs(secs) + Duration::from_millis(millis);
            let next_id = write_guard.next_id;
            let sender_channel = write_guard.channel.sender.clone();
            write_guard.listeners.insert(next_id, listener);
            let handle = tokio::task::spawn (async move {
                let id = next_id;
                tokio::time::sleep(duration_until_start).await;
                let mut interval = time::interval(duration_between_repeats);
                loop {
                    interval.tick().await;
                    let Ok(()) = sender_channel.send(id).await else {
                        break;
                    };
                }
            });
            write_guard.tasks.insert(next_id, handle);
            result.get().set_id(next_id);
            result.get().set_cancelable(capnp_rpc::new_client(CancelableImpl{id: next_id, rc: this.clone()}));
            write_guard.next_id += 1;
            drop(write_guard);
            return Result::Ok(())
        });
    }
}

fn repeat_helper(dur: Duration, id: u8, sender_channel: tokio::sync::mpsc::Sender<u8>) -> JoinHandle<()> {
    let handle = tokio::task::spawn(async move {
        let mut interval = time::interval(dur);
        loop {
            interval.tick().await;
            let Ok(()) = sender_channel.send(id).await else {
                break;
            };
        }
    });
    return handle
}
*//* 
struct CancelableImpl {
    id: u8,
    key: Key,
    //rc: Rc<tokio::sync::RwLock<SchedulerImpl>>
    rc: Rc<tokio::sync::Mutex<Scheduler2>>
}*/
/*
impl cancelable::Server for CancelableImpl {
    fn cancel(&mut self, params: cancelable::CancelParams, _: cancelable::CancelResults) -> Promise<(), Error> {
        return tokio::task::block_in_place(move || { 
            let mut write_guard = self.rc.blocking_lock();
            
            let Some(handle) = write_guard.task_handles.get(&self.id) else {
                return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Task no longer running")});
            };
            handle.abort();
            write_guard.listeners.remove(&self.id);
            drop(write_guard);
            
            return Promise::<(), Error>::ok(());
        });
    }
}*/
/*
struct ListenerImpl<T, P, V> {
    client: T,
    params: P,
    requests: Vec<Box<dyn Fn(&T, P) -> Result<V, Error>>>,
    results: Vec<Result<V, Error>>
}

impl <T : capnp::capability::FromClientHook + Clone, P : Clone, V>listener::Server for ListenerImpl<T, P, V> {
    fn event(&mut self, params: listener::EventParams, _: listener::EventResults) -> Promise<(), Error> {
        //let params_reader = pry!(params.get());
        //let id = params_reader.get_id();
        //print!("{id}");
        for request in &self.requests {
            self.results.push(request(&self.client, self.params.clone()));
        }
        
        Promise::ok(())
    }
}*/
#[derive(Clone)]
pub struct ListenerTestImpl{
    pub test: Vec<u8>
}
impl listener_test::Server for ListenerTestImpl {
    fn do_stuff(&mut self, params: listener_test::DoStuffParams, mut result: listener_test::DoStuffResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let id = params_reader.get_test();
        println!("{id}");
        self.test.push(id);
        result.get().set_result(id);
        Promise::ok(())
    }
    fn retrieve_test_vec(&mut self, _: listener_test::RetrieveTestVecParams, mut result: listener_test::RetrieveTestVecResults) -> Promise<(), Error> {
        result.get().set_result(self.test.clone().as_slice());
        Promise::ok(())
    }
}
/* 
impl listener_test::Server for Rc<RefCell<ListenerTestImpl>> {
    fn do_stuff(&mut self, params: listener_test::DoStuffParams, mut result: listener_test::DoStuffResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let id = params_reader.get_test();
        println!("{id}");
        self.borrow_mut().test.push(id);
        result.get().set_result(id);
        Promise::ok(())
    }
    fn retrieve_test_vec(&mut self, _: listener_test::RetrieveTestVecParams, mut result: listener_test::RetrieveTestVecResults) -> Promise<(), Error> {
        result.get().set_result(self.borrow().test.clone().as_slice());
        Promise::ok(())
    }
}*/
/* 
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_test() -> eyre::Result<()> {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(100);
    let scheduler_rc = Rc::new(tokio::sync::RwLock::new(SchedulerImpl{tasks: HashMap::new(), next_id: 0, channel: Channel{sender: sender}, listeners: HashMap::new()}));
    let scheduler: scheduler::Client = capnp_rpc::new_client(scheduler_rc.clone());
    let listener_test_vec = Rc::new(RefCell::new(Vec::new()));
    let listener_test: listener_test::Client = capnp_rpc::new_client(Rc::new(RefCell::new(ListenerTestImpl{test: listener_test_vec.clone()})));
    let closure = Box::new(|c: &listener_test::Client, p: u8| -> Result<u8, Error> {
        let mut request = c.do_stuff_request();
        request.get().set_test(p);
        return Ok(tokio::task::block_in_place(move || {tokio::runtime::Handle::current().block_on(request.send().promise)})?.get()?.get_result())
    });
    let listener: listener::Client = capnp_rpc::new_client(ListenerImpl{client: listener_test.clone(), params: 1, requests: vec!(closure.clone()), results: Vec::new()});
    let listener2: listener::Client = capnp_rpc::new_client(ListenerImpl{client: listener_test.clone(), params: 2, requests: vec!(closure), results: Vec::new()});

    let mut repeat_request = scheduler.repeat_request();
    let mut builder = repeat_request.get().init_delay();
    builder.set_secs(1);
    builder.set_millis(0);
    repeat_request.get().set_listener(listener.clone());
    let id = futures::executor::block_on(repeat_request.send().promise)?.get()?.get_id();
    //print!("{}", id);
    let mut repeat_request = scheduler.repeat_request();
    let mut builder = repeat_request.get().init_delay();
    builder.set_secs(2);
    builder.set_millis(0);
    repeat_request.get().set_listener(listener2);
    let id = futures::executor::block_on(repeat_request.send().promise)?.get()?.get_id();
    //print!("{}", id);

    let mut poll_rc = scheduler_rc.clone();
    let mut system_time = std::time::SystemTime::now();
    let duration = Duration::from_secs(20);
    let mut done = false;
    loop {
        if system_time.elapsed().unwrap() > duration {break;}
        if done == false {
            tokio::join!(
                create_and_send_test_request(listener_test.clone(), scheduler.clone()),
                poll_scheduler(&mut poll_rc, &mut receiver),
                create_and_send_test_request(listener_test.clone(), scheduler.clone())
            );
            done = true
        }
        poll_scheduler(&mut poll_rc, &mut receiver).await;
    }
    println!("test vec length: {}", listener_test_vec.borrow().len());
    if listener_test_vec.borrow().len() > 40 {
        return Ok(());
    } else {
        return Err(eyre::eyre!("not enough responses recieved"));
    }
}

async fn poll_scheduler(poll_rc: &Rc<tokio::sync::RwLock<SchedulerImpl>>, receiver: &mut tokio::sync::mpsc::Receiver<u8>) {
    if let Some(i) = receiver.recv().await {
        let guard = poll_rc.read().await;
        if let Some(client) = guard.listeners.get(&i) {
            let mut request = client.event_request();
            request.get().set_id(i);
            futures::executor::block_on(request.send().promise).unwrap();
        }
        drop(guard);
    }
    
}


async fn create_and_send_test_request(listener_test: listener_test::Client, scheduler: scheduler::Client) -> capnp::capability::Response<crate::scheduler_capnp::scheduler::repeat_results::Owned> {
    let closure = Box::new(|c: &listener_test::Client, p: u8| -> Result<u8, Error> {
        let mut request = c.do_stuff_request();
        request.get().set_test(p);
        return Ok(tokio::task::block_in_place(move || {tokio::runtime::Handle::current().block_on(request.send().promise)})?.get()?.get_result())
    });
    let listener3: listener::Client = capnp_rpc::new_client(ListenerImpl{client: listener_test, params: 4, requests: vec!(closure), results: Vec::new()});
    let mut repeat_request = scheduler.repeat_request();
    let mut builder = repeat_request.get().init_delay();
    builder.set_secs(4);
    builder.set_millis(0);
    repeat_request.get().set_listener(listener3);
    return repeat_request.send().promise.await.unwrap();
}
*//* 
impl <T : capnp::capability::FromClientHook + Clone, P : Clone, V>saveable::Server for ListenerImpl<T, P, V> {
    fn save(&mut self, _: keystone::sturdyref_capnp::saveable::SaveParams, mut result: keystone::sturdyref_capnp::saveable::SaveResults) -> Promise<(), Error> {
        let underlying_save_request = self.client.clone().cast_to::<crate::sturdyref_capnp::saveable::Client>().save_request();
        let saved = futures::executor::block_on(underlying_save_request.send().promise).unwrap();
        let underlying_sturdyref = saved.get().unwrap().get_value();
        let slice = underlying_sturdyref.get_as::<&[u8]>().unwrap();
        let sturdyref = crate::sturdyref::Saved::Listener(slice.to_vec());
        let Ok(signed_row) = crate::sturdyref::save_sturdyref(sturdyref) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to save sturdyref")});
        };
        let Ok(()) = result.get().init_value().set_as(signed_row.as_slice()) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to save sturdyref")});
        };
        Promise::ok(())
    }
}*/

pub trait Listener {
    fn send_requests(&mut self, id: u8);
    fn save(&mut self) -> eyre::Result<Vec<u8>>;
}

impl saveable::Server for ListenerTestImpl {
    fn save(&mut self, _: keystone::sturdyref_capnp::saveable::SaveParams, mut result: keystone::sturdyref_capnp::saveable::SaveResults) -> Promise<(), Error> {
        let sturdyref = crate::sturdyref::Saved::ListenerTest(self.test.clone());
        let Ok(signed_row) = crate::sturdyref::save_sturdyref(sturdyref) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to save sturdyref")});
        };
        let Ok(()) = result.get().init_value().set_as(signed_row.as_slice()) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to save underlying sturdyref")});
        };
        Promise::ok(())
    }
}

impl Listener for listener_test::Client {
    fn send_requests(&mut self, id: u8) {
        let mut request = self.do_stuff_request();
        match id {
            _ => {
                request.get().set_test(id);
                tokio::task::block_in_place(move ||tokio::runtime::Handle::current().block_on(request.send().promise).unwrap().get().unwrap().get_result());
            }
        }
    }

    fn save(&mut self) -> eyre::Result<Vec<u8>> {
        let result = tokio::task::block_in_place(move ||tokio::runtime::Handle::current().block_on(self.clone().cast_to::<saveable::Client>().save_request().send().promise))?;
        let underlying_sturdyref = result.get()?.get_value().get_as::<&[u8]>()?;
        let sturdyref = crate::sturdyref::Saved::Listener(underlying_sturdyref.to_vec());
        let Ok(signed_row) = crate::sturdyref::save_sturdyref(sturdyref) else {
            return Err(eyre::eyre!("Failed to save sturdyref"));
        };
        return Ok(signed_row)   
    }
}
/* 
impl Listener for Rc<RefCell<ListenerTestImpl>> {
    fn send_requests(&mut self, id: u8) {
        let mut client: listener_test::Client = capnp_rpc::new_client(self.clone());
        let mut request = client.do_stuff_request();
        match id {
            _ => {
                request.get().set_test(id);
                tokio::task::block_in_place(move ||tokio::runtime::Handle::current().block_on(request.send().promise).unwrap().get().unwrap().get_result());
            }
        }
    }
    fn save(&mut self) -> eyre::Result<Vec<u8>> {
        let cloned = self.borrow_mut().clone();
        let mut client: saveable::Client = capnp_rpc::new_client(cloned);
        let result = futures::executor::block_on(client.save_request().send().promise)?;
        let underlying_sturdyref = result.get()?.get_value().get_as::<&[u8]>()?;
        let sturdyref = crate::sturdyref::Saved::Listener(underlying_sturdyref.to_vec());
        let Ok(signed_row) = crate::sturdyref::save_sturdyref(sturdyref) else {
            return Err(eyre::eyre!("Failed to save sturdyref"));
        };
        return Ok(signed_row)   
    }
}*/
impl crate::sturdyref_capnp::saveable::Server for Box<dyn Listener> {
    fn save(&mut self, _: crate::sturdyref_capnp::saveable::SaveParams, mut result: crate::sturdyref_capnp::saveable::SaveResults) -> Promise<(), Error> {
        let Ok(signed_row) = self.as_mut().save() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to save underlying sturdyref")});
        };
        let Ok(()) = result.get().init_value().set_as(signed_row.as_slice()) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to save underlying sturdyref")});
        };
        Promise::ok(())
    }
}
impl listener::Server for Box<dyn Listener> {
    fn event(&mut self, params: listener::EventParams, _: listener::EventResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let id = params_reader.get_id();
        self.send_requests(id);
        Promise::ok(())
    }
}
/* 
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_test() -> eyre::Result<()> {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(100);
    let scheduler_rc = Rc::new(tokio::sync::RwLock::new(SchedulerImpl{tasks: HashMap::new(), next_id: 1, channel: Channel{sender: sender}, listeners: HashMap::new(), sturdyrefs: HashMap::new()}));
    let scheduler: scheduler::Client = capnp_rpc::new_client(scheduler_rc.clone());
    //let listener_test_rc = Rc::new(RefCell::new(ListenerTestImpl{test: Vec::new()}));
    let listener_test: listener_test::Client = capnp_rpc::new_client(ListenerTestImpl{test: Vec::new()});
    let id = create_and_send_test_request(Box::new(listener_test.clone()) as Box<dyn Listener>, scheduler.clone(), 1).await;
    let id = create_and_send_test_request(Box::new(listener_test.clone()) as Box<dyn Listener>, scheduler.clone(), 2).await;
    let mut poll_rc = scheduler_rc.clone();
    let mut system_time = std::time::SystemTime::now();
    let duration = Duration::from_secs(20);
    let mut done = false;
    loop {
        if system_time.elapsed().unwrap() > duration {break;}
        if done == false {
            tokio::join!(
                create_and_send_test_request(Box::new(listener_test.clone()) as Box<dyn Listener>, scheduler.clone(), 4),
                poll_scheduler(&mut poll_rc, &mut receiver),
                create_and_send_test_request(Box::new(listener_test.clone()) as Box<dyn Listener>, scheduler.clone(), 4)
            );
            done = true
        }
        poll_scheduler(&mut poll_rc, &mut receiver).await;
    }
    //let listener: listener_test::Client = capnp_rpc::new_client(listener_test_rc);
    let result = futures::executor::block_on(listener_test.retrieve_test_vec_request().send().promise)?;
    let test = result.get()?.get_result()?;
    println!("test vec length: {}", test.len());
    if test.len() >= 40 {
        return Ok(());
    } else {
        return Err(eyre::eyre!("not enough responses recieved"));
    }
}

async fn poll_scheduler(poll_rc: &Rc<tokio::sync::RwLock<SchedulerImpl>>, receiver: &mut tokio::sync::mpsc::Receiver<u8>) {
    if let Some(i) = receiver.recv().await {
        let guard = poll_rc.read().await;
        if let Some(client) = guard.listeners.get(&i) {
            let mut request = client.event_request();
            request.get().set_id(i);
            futures::executor::block_on(request.send().promise).unwrap();
        }
        drop(guard);
    }
}
async fn create_and_send_test_request(listener_test: Box<dyn Listener>, scheduler: scheduler::Client, every_secs: u64) -> u8 {
    let listener: listener::Client = capnp_rpc::new_client(listener_test);
    let mut repeat_request = scheduler.repeat_request();
    let mut builder = repeat_request.get().init_delay();
    builder.set_secs(every_secs);
    builder.set_millis(0);
    repeat_request.get().set_listener(listener);
    let id = repeat_request.send().promise.await.unwrap().get().unwrap().get_id();
    return id;
}
*/
#[derive(Serialize, Deserialize)]
struct Scheduled {
    next: i64,
    every: Option<Every>,
    missed_event_behaviour: MissedEventBehaviour
}

impl Scheduled {
    fn update(&mut self) -> Option<i64> {
        let Some(every) = &self.every else {
            return None
        };
        let tz: Tz = every.tz_identifier.parse().unwrap();
        let dt = tz.timestamp_millis_opt(self.next).earliest().unwrap();
        let dt = dt.checked_add_months(chrono::Months::new(every.months)).unwrap();
        let dt = dt.checked_add_days(chrono::Days::new(every.days)).unwrap();
        let dt = dt.checked_add_signed(chrono::Duration::hours(every.hours) + chrono::Duration::minutes(every.mins) + chrono::Duration::seconds(every.secs) + chrono::Duration::milliseconds(every.millis)).unwrap();
        self.next = dt.timestamp_millis();
        return Some(dt.timestamp_millis());
    }
}

#[derive(Serialize, Deserialize)]
enum MissedEventBehaviour {
    SendAll
}

#[derive(Serialize, Deserialize)]
struct Every {
    months: u32,
    days: u64,
    hours: i64,
    mins: i64,
    secs: i64,
    millis: i64,
    tz_identifier: String
}

pub struct SchedulerImpl {
    scheduled: HashMap<u8, Scheduled>,
    next_id: u8,
    queue: tokio_util::time::delay_queue::DelayQueue<u8>,
    keys: HashMap<u8, tokio_util::time::delay_queue::Key>,
    listeners: HashMap<u8, crate::scheduler_capnp::listener::Client>,
    sturdyrefs: HashMap<u8, Vec<u8>>
}

thread_local! (
    pub static SCHEDULER_SET: RefCell<CapabilityServerSet<Rc<tokio::sync::Mutex<SchedulerImpl>>, scheduler::Client>> = RefCell::new(CapabilityServerSet::new());
);

impl scheduler::Server for Rc<tokio::sync::Mutex<SchedulerImpl>> {
    fn schedule_for(&mut self, params: scheduler::ScheduleForParams, mut result: scheduler::ScheduleForResults) -> Promise<(), Error> {
        let this = self.clone();
        let params_reader = pry!(params.get());
        let listener = pry!(params_reader.get_listener());
        let start_timestamp = params_reader.get_schedule_for_unix_timestamp_millis();
        return Promise::from_future(async move {
            let mut guard = this.lock().await;
            let next_id = guard.next_id;
            guard.scheduled.insert(next_id, Scheduled{next: start_timestamp, every: None, missed_event_behaviour: MissedEventBehaviour::SendAll});
            guard.listeners.insert(next_id, listener);
            let Some(dur) = Duration::from_millis(start_timestamp as u64).checked_sub(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap()) else {
                return Err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Start timestamp before current time")});
            };
            let key = guard.queue.insert(next_id, dur);
            guard.keys.insert(next_id, key);

            result.get().set_id(next_id);
            result.get().set_cancelable(capnp_rpc::new_client(CancelableImpl{id: next_id, rc: this.clone()}));
            guard.next_id += 1;
            drop(guard);
            return Result::Ok(())
        });
    }
    fn schedule_for_then_repeat(&mut self, params: scheduler::ScheduleForThenRepeatParams, mut result: scheduler::ScheduleForThenRepeatResults) -> Promise<(), Error> {
        let this = self.clone();
        let params_reader = pry!(params.get());
        let listener = pry!(params_reader.get_listener());
        let start_timestamp = params_reader.get_start_unix_timestamp_millis();
        capnp_let!({every : {months, days, hours, mins, secs, millis, tz_identifier}} = params_reader);
        let tz = tz_identifier.to_string();
        return Promise::from_future(async move {
            let mut guard = this.lock().await;
            let next_id = guard.next_id;
            guard.scheduled.insert(next_id, Scheduled{next: start_timestamp, every: Some(Every { months: months, days: days, hours: hours, mins: mins, secs: secs, millis: millis, tz_identifier: tz }), missed_event_behaviour: MissedEventBehaviour::SendAll});
            guard.listeners.insert(next_id, listener);
            let Some(dur) = Duration::from_millis(start_timestamp as u64).checked_sub(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap()) else {
                return Err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Start timestamp before current time")});
            };
            let key = guard.queue.insert(next_id, dur);
            guard.keys.insert(next_id, key);

            result.get().set_id(next_id);
            result.get().set_cancelable(capnp_rpc::new_client(CancelableImpl{id: next_id, rc: this.clone()}));
            guard.next_id += 1;
            drop(guard);
            return Result::Ok(())
        });
    }
}

struct CancelableImpl {
    id: u8,
    rc: Rc<tokio::sync::Mutex<SchedulerImpl>>
}

impl cancelable::Server for CancelableImpl {
    fn cancel(&mut self, _: cancelable::CancelParams, _: cancelable::CancelResults) -> Promise<(), Error> {
        let this = self.rc.clone();
        let id = self.id.clone();
        return Promise::from_future(async move  {
            let mut guard = this.lock().await;
            let Some(key) = guard.keys.remove(&id) else {
                return Err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Task no longer running")});
            };
            guard.queue.remove(&key);
            drop(guard);
            
            return Result::Ok(())
        });
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler2_test() -> eyre::Result<()> {
    let scheduler_rc = Rc::new(tokio::sync::Mutex::new(SchedulerImpl{scheduled: HashMap::new(), queue: DelayQueue::new(), keys: HashMap::new(), next_id: 1, listeners: HashMap::new(), sturdyrefs: HashMap::new()}));
    let scheduler: scheduler::Client = capnp_rpc::new_client(scheduler_rc.clone());
    let listener_test: listener_test::Client = capnp_rpc::new_client(ListenerTestImpl{test: Vec::new()});
    let start = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64 + 2000;
    create_and_send_test_request(Box::new(listener_test.clone()) as Box<dyn Listener>, scheduler.clone(), 1, start).await;
    create_and_send_test_request(Box::new(listener_test.clone()) as Box<dyn Listener>, scheduler.clone(), 2, start).await;
    let mut poll_rc = scheduler_rc.clone();
    let system_time = std::time::SystemTime::now();
    let duration = Duration::from_secs(22);
    let mut done = false;
    loop {
        if system_time.elapsed().unwrap() >= duration {break;}
        if done == false {
            tokio::join!(
                create_and_send_test_request(Box::new(listener_test.clone()) as Box<dyn Listener>, scheduler.clone(), 4, start),
                poll_scheduler(&mut poll_rc),
                create_and_send_test_request(Box::new(listener_test.clone()) as Box<dyn Listener>, scheduler.clone(), 4, start)
            );
            done = true
        }
        poll_scheduler(&mut poll_rc).await;
    }
    let result = futures::executor::block_on(listener_test.retrieve_test_vec_request().send().promise)?;
    let test = result.get()?.get_result()?;
    println!("test vec length: {}", test.len());
    if test.len() >= 40 {
        return Ok(());
    } else {
        return Err(eyre::eyre!("not enough responses recieved"));
    }
}

async fn create_and_send_test_request(listener_test: Box<dyn Listener>, scheduler: scheduler::Client, every_secs: u64, start: i64) -> u8 {
    let listener: listener::Client = capnp_rpc::new_client(listener_test);
    let mut repeat_request = scheduler.schedule_for_then_repeat_request();
    let mut builder_every = repeat_request.get().init_every();
    builder_every.set_months(0);
    builder_every.set_days(0);
    builder_every.set_hours(0);
    builder_every.set_mins(0);
    builder_every.set_secs(every_secs as i64);
    builder_every.set_millis(0);
    builder_every.set_tz_identifier("EST");
    repeat_request.get().set_start_unix_timestamp_millis(start);

    repeat_request.get().set_listener(listener);
    let id = repeat_request.send().promise.await.unwrap().get().unwrap().get_id();
    return id;
}

async fn poll_scheduler(poll_rc: &mut Rc<tokio::sync::Mutex<SchedulerImpl>>) {
        tokio::task::yield_now().await;
        let mut guard = poll_rc.lock().await;
        //guard.queue.insert(value, timeout)
        if let Some(i) = guard.queue.next().await {
            let i = i.into_inner();
            if let Some(client) = guard.listeners.get(&i) {
                let mut request = client.event_request();
                request.get().set_id(i.clone());
                futures::executor::block_on(request.send().promise).unwrap();
            }
            let Some(scheduled) = guard.scheduled.get_mut(&i) else {
                todo!()
            };
            
            if let Some(next) = scheduled.update() {
                let dur = Duration::from_millis(next as u64) - std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
                let key = guard.queue.insert(i.clone(), dur);
                guard.keys.insert(i, key);
            };
        }
        //guard.tasks;
        drop(guard);
}