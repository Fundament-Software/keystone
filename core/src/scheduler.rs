use std::{collections::HashMap, time::Duration, cell::RefCell, rc::Rc, sync::{Arc, RwLock}};

use capnp::{capability::{Promise, Request}, Error};
use capnp_macros::capnp_let;
use capnp_rpc::pry;
use tokio::{task::{JoinHandle, self}, time};
use capnp::IntoResult;

use crate::{scheduler_capnp::{scheduler, listener, get_scheduler, listener_test}, cap_std_capnp::cap_fs, cap_std_capnproto};

struct GetSchedulerImpl {

}

impl get_scheduler::Server for GetSchedulerImpl {
    fn get(&mut self, params: get_scheduler::GetParams, mut result: get_scheduler::GetResults) -> Promise<(), Error> {
        todo!()
    }
}

struct Channel {
    sender: tokio::sync::mpsc::Sender<u8>,
    //receiver: tokio::sync::mpsc::Receiver<u8>
}

struct SchedulerImpl {
    tasks: HashMap<u8, JoinHandle<()>>,
    next_id: u8,
    channel: Channel,
    listeners: HashMap<u8, crate::scheduler_capnp::listener::Client>
}

impl scheduler::Server for Rc<tokio::sync::RwLock<SchedulerImpl>> {
    fn repeat(&mut self, params: scheduler::RepeatParams, mut result: scheduler::RepeatResults) -> Promise<(), Error> {
        return tokio::task::block_in_place(move || { 
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
        write_guard.next_id += 1;
        drop(write_guard);
        
        return Promise::<(), Error>::ok(());
    });
        Promise::ok(())
    }
}

fn repeat_helper(dur: Duration, id: u8, sender_channel: tokio::sync::mpsc::Sender<u8>) -> JoinHandle<()> {
    let handle = tokio::task::spawn(async move {
        let mut interval = time::interval(dur);
        loop {
            interval.tick().await;
            let Ok(()) = sender_channel.send(id).await else {
                todo!()
            };
        }
    });
    return handle
}

struct ListenerImpl<T, P, V> {
    client: T,
    params: P,
    requests: Vec<Box<dyn Fn(&T, P) -> Result<V, Error>>>,
    results: Vec<Result<V, Error>>
}

impl <T, P: Clone, V>listener::Server for ListenerImpl<T, P, V> {
    fn event(&mut self, params: listener::EventParams, _: listener::EventResults) -> Promise<(), Error> {
        //let params_reader = pry!(params.get());
        //let id = params_reader.get_id();
        //print!("{id}");
        for request in &self.requests {
            self.results.push(request(&self.client, self.params.clone()));
        }
        
        Promise::ok(())
    }
}

struct ListenerTestImpl{
    test: Rc<RefCell<Vec<u8>>>
}

impl listener_test::Server for ListenerTestImpl {
    fn do_stuff(&mut self, params: listener_test::DoStuffParams, mut result: listener_test::DoStuffResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let test = params_reader.get_test();
        println!("{test}");
        self.test.borrow_mut().push(test);
        result.get().set_result(test);
        Promise::ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_test() -> eyre::Result<()> {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(100);
    let scheduler_rc = Rc::new(tokio::sync::RwLock::new(SchedulerImpl{tasks: HashMap::new(), next_id: 0, channel: Channel{sender: sender}, listeners: HashMap::new()}));
    let scheduler: scheduler::Client = capnp_rpc::new_client(scheduler_rc.clone());
    let listener_test_vec = Rc::new(RefCell::new(Vec::new()));
    let listener_test: listener_test::Client = capnp_rpc::new_client(ListenerTestImpl{test: listener_test_vec.clone()});
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

    let poll_rc = scheduler_rc.clone();
    let mut done = false;
    let mut system_time = std::time::SystemTime::now();
    let duration = Duration::from_secs(20);
    loop {
        if system_time.elapsed().unwrap() > duration {break;}
        let guard = poll_rc.read().await;
        
        if let Some(i) = receiver.recv().await {
            if let Some(client) = guard.listeners.get(&i) {
                let mut request = client.event_request();
                request.get().set_id(i);
                futures::executor::block_on(request.send().promise)?;
            }
        }
    
        
        
        drop(guard);
        do_once(done, listener_test.clone(), scheduler.clone());
        done = true;
        //let test = test_rc.read().unwrap().clone();
        //for int in test {
        //    print!("{int}");
        //}
    }
    if listener_test_vec.borrow().len() > 32 {
        return Ok(());
    } else {
        return Err(eyre::eyre!("not enough responses recieved"));
    }
}

fn do_once(done: bool, listener_test: listener_test::Client, scheduler: scheduler::Client) {
    if done == false {
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
        let id = futures::executor::block_on(repeat_request.send().promise).unwrap().get().unwrap().get_id();
    }

}