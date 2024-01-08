
use std::{collections::HashMap, cell::{RefCell, Cell, OnceCell}, path::PathBuf, rc::Rc, time::Duration};
use capnp::{capability::{Promise, FromClientHook}, Error};
use capnp_rpc::pry;
use capnp::private::capability::ClientHook;
use keystone::sturdyref_capnp::saveable;
use serde::{Serialize, Deserialize};
use signature::Signer;
use tokio_util::time::DelayQueue;
use crate::{sturdyref_capnp::restorer, scheduler_capnp::{scheduler, listener, listener_test}, cap_std_capnp::{ambient_authority, dir}, cap_std_capnproto::{self, DirImpl, AmbientAuthorityImpl}, scheduler::{Listener, ListenerTestImpl, SchedulerImpl, Scheduled, MissedEventBehaviour}};
use rand::rngs::OsRng;
use ed25519_dalek::{SigningKey, SignatureError};
use ed25519_dalek::Signature;
//TODO Replace with a database

thread_local!(
    static NEXT_ROW: Cell<u8> = Cell::new(0);
    static STURDYREFS: RefCell<HashMap<u8, String>> = RefCell::new(HashMap::new());
    static SIGNING_KEY: OnceCell<SigningKey> = OnceCell::new();
);
#[derive(Serialize, Deserialize)]
pub enum Saved {
    Dir(PathBuf),
    Scheduler(Vec<(u8, Scheduled)>, Vec<(u8, Vec<u8>)>),
    Listener(Vec<u8>),
    ListenerTest(Vec<u8>),
}

struct RestorerImpl;

impl restorer::Server for RestorerImpl {
    fn restore(&mut self, params: restorer::RestoreParams, mut result: restorer::RestoreResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let signed = pry!(params_reader.get_value().get_as::<&[u8]>());
        let Ok(key) = verify(signed) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to verify sturdyref authenticity")});
        };
        let Ok(sturdyref) = get_sturdyref(&key) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to find saved sturdyref")});
        };
        let Ok(cap) = restore_helper(sturdyref) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to restore underlying object")});
        };
        result.get().init_cap().set_as_capability(cap);
        Promise::ok(())
    }
    fn delete(&mut self, params: restorer::DeleteParams, _: restorer::DeleteResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let signed = pry!(params_reader.get_value().get_as::<&[u8]>());
        let Ok(key) = verify(signed) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to verify sturdyref authenticity")});
        };
        let Some(()) = delete_sturdyref(&key) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to find correspodning sturdyref")});
        };
        return Promise::ok(())
    }
}

fn restore_helper(saved: Saved) -> eyre::Result<Box<dyn ClientHook>> {
    match saved {
        Saved::Dir(path) => {
            let dir = cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority())?;
            let cap: dir::Client = cap_std_capnproto::DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: dir}));
            return Ok(cap.into_client_hook());
        }
        Saved::Listener(key) => {
            let verified = verify(key.as_slice())?;
            let sturdyref = get_sturdyref(&verified)?;
            let cap = restore_as_listener_helper(sturdyref)?.into_client_hook();
            //let listener: listener::Client = capnp_rpc::new_client(Box::new(cap) as Box<dyn Listener>);
            return Ok(cap);
        }
        Saved::ListenerTest(test_vec) => {
            let cap: listener_test::Client = capnp_rpc::new_client(ListenerTestImpl{test: test_vec});
            return Ok(cap.into_client_hook())
        },
        Saved::Scheduler(scheduled_vec, listener_sturdyref_vec) => {
            //TODO a bit weird with cancelable
            let mut sc = SchedulerImpl{scheduled: HashMap::new(), next_id: 0, queue: DelayQueue::new(), keys: HashMap::new(), listeners: HashMap::new(), sturdyrefs: HashMap::new()};
            for mut scheduled in scheduled_vec {
                match scheduled.1.missed_event_behaviour {
                    MissedEventBehaviour::SendAll => {
                        while let None = Duration::from_millis(scheduled.1.next as u64).checked_sub(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap()) {
                            let key = sc.queue.insert(scheduled.0, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap());
                            sc.keys.insert(scheduled.0, key);
                            scheduled.1.update();
                        }
                    },
                }
                sc.scheduled.insert(scheduled.0, scheduled.1);
                if scheduled.0 >= sc.next_id {
                    sc.next_id = scheduled.0 + 1;
                }
            }
            for unverified_sturdyref in listener_sturdyref_vec {
                sc.sturdyrefs.insert(unverified_sturdyref.0, unverified_sturdyref.1.clone());
                let verified = verify(unverified_sturdyref.1.as_slice())?;
                let sturdyref = get_sturdyref(&verified)?;
                let cap: listener::Client = restore_as_listener_helper(sturdyref)?;
                sc.listeners.insert(unverified_sturdyref.0, cap);
            }
            let cap: scheduler::Client = crate::scheduler::SCHEDULER_SET.with_borrow_mut(|set| set.new_client(Rc::new(tokio::sync::Mutex::new(sc))));
            return Ok(cap.into_client_hook());
        }
    }
}
//TODO this is probably possible to replace with 1 function
fn restore_as_listener_helper(saved: Saved) -> eyre::Result<listener::Client> {
    match saved {
        Saved::ListenerTest(test_vec) => {
            let listener_test_cap: listener_test::Client = capnp_rpc::new_client(ListenerTestImpl{test: test_vec});
            let cap: listener::Client = capnp_rpc::new_client(Box::new(listener_test_cap) as Box<dyn Listener>);
            return Ok(cap)
        },
        _ => Err({eyre::eyre!("Restore as listener not implemented for underlying cap")})
    }
}

//TODO save the signing key in database/file
pub fn sign(row: u8) -> Vec<u8> {
    return SIGNING_KEY.with(|key| {
        let signing_key = key.get_or_init(|| {
            return SigningKey::generate(&mut OsRng);
        });
        let mut vec = vec![row];
        let signature = signing_key.sign(vec.as_slice());
        vec.extend_from_slice(signature.to_bytes().as_slice());
        return vec
    });
}

fn verify(signed: &[u8]) -> Result<u8, SignatureError> {
    return SIGNING_KEY.with(|key| {
        let signing_key = key.get_or_init(|| {
            return SigningKey::generate(&mut OsRng);
        });
        let (m, s) = signed.split_at(1);
        if let Err(err) = signing_key.verify(m, &Signature::from_slice(s)?) {
            return Err(err);
        };
        return Ok(m[0])
    });
}

fn get_sturdyref(key: &u8) -> eyre::Result<Saved> {
    return Ok(serde_json::from_str(STURDYREFS.with_borrow_mut(|map| map.remove(key)).ok_or_else(|| eyre::eyre!("Failed to find corresponding sturdyref"))?.as_str())?);
}

fn delete_sturdyref(key: &u8) -> Option<()> {
    //TODO make more generic/save to database
    STURDYREFS.with_borrow_mut(|map| map.remove(key))?;
    return Some(())
}

pub fn save_sturdyref(sturdyref: Saved) -> eyre::Result<Vec<u8>> {
    //TODO make more generic/save to database
    let key = NEXT_ROW.get();
    NEXT_ROW.replace(key + 1);
    let serialized = serde_json::to_string(&sturdyref)?;
    STURDYREFS.with_borrow_mut(|map| map.insert(key, serialized));
    return Ok(sign(key));
}

#[test]
fn sturdyref_dir_test() -> eyre::Result<()> {
    use crate::cap_std_capnproto;
    use crate::sturdyref_capnp::saveable;
    use crate::sturdyref_capnp::saveable::Server;

    let ambient_authority: ambient_authority::Client = capnp_rpc::new_client(AmbientAuthorityImpl{});
        
    let mut open_ambient_request = ambient_authority.dir_open_ambient_request();
    open_ambient_request.get().set_path(std::env::temp_dir().to_str().unwrap());
    let dir = futures::executor::block_on(open_ambient_request.send().promise)?.get()?.get_result()?;

    let metadata = futures::executor::block_on(dir.dir_metadata_request().send().promise)?.get()?.get_metadata()?;
    cap_std_capnproto::tests::test_metadata(metadata);

    let request = dir.cast_to::<crate::sturdyref_capnp::saveable::Client>().save_request();
    let sturdyref = futures::executor::block_on(request.send().promise)?;
    
    let restorer: crate::sturdyref_capnp::restorer::Client = capnp_rpc::new_client(RestorerImpl);
    let mut restore_request = restorer.restore_request();
    restore_request.get().init_value().set_as(sturdyref.get()?.get_value())?;//set_value(sturdyref);
    let restored_dir = futures::executor::block_on(restore_request.send().promise)?.get()?.get_cap()?.get_as_capability::<crate::cap_std_capnp::dir::Client>()?;

    let metadata = futures::executor::block_on(restored_dir.dir_metadata_request().send().promise)?.get()?.get_metadata()?;
    cap_std_capnproto::tests::test_metadata(metadata)?;

    return Ok(())
}
