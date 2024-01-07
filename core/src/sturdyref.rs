
use std::{collections::HashMap, cell::{RefCell, Cell, OnceCell}, path::PathBuf, rc::Rc, time::Duration};
use capnp::{capability::{Promise, FromClientHook}, Error};
use capnp_rpc::pry;
use capnp::private::capability::ClientHook;
use keystone::sturdyref_capnp::saveable;
use serde::{Serialize, Deserialize};
use signature::Signer;
use crate::{sturdyref_capnp::restorer, cap_std_capnp::{ambient_authority, dir}, cap_std_capnproto::{self, DirImpl, AmbientAuthorityImpl}};
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
    restore_request.get().init_value().set_as(sturdyref.get()?.get_value())?;
    let restored_dir = futures::executor::block_on(restore_request.send().promise)?.get()?.get_cap()?.get_as_capability::<crate::cap_std_capnp::dir::Client>()?;

    let metadata = futures::executor::block_on(restored_dir.dir_metadata_request().send().promise)?.get()?.get_metadata()?;
    cap_std_capnproto::tests::test_metadata(metadata)?;

    return Ok(())
}
