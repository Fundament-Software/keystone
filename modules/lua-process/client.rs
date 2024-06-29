use crate::hello_world_capnp::hello_world;
use crate::hello_world_capnp::hello_front;
use capnp::capability::FromClientHook;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use std::net::ToSocketAddrs;
use std::process;
use std::rc::Rc;
use std::sync::Arc;
use std::error::Error;
use std::error::Error as StdError;
use futures::AsyncReadExt;
use mlua::prelude::*;
use mlua::{Lua, UserData, UserDataMethods};
use mlua::Result as LuaResult;
use mlua::{FromLua, Error as LuaError};
use mlua::Thread;
use eyre::{eyre, Result};

pub struct Client {
    hello_front: hello_front::Client,
}

impl UserData for hello_world::Client {}

impl<'lua> FromLua<'lua> for hello_world::Client {
    fn from_lua(value: LuaValue<'lua>, _lua: &'lua Lua) -> Result<Self, LuaError> {
        if let LuaValue::UserData(ref ud) = value {
            if let Ok(client) = ud.borrow::<hello_world::Client>() {
                return Ok(client.clone());
            }
        }

        Err(LuaError::FromLuaConversionError {
            from: value.type_name(),
            to: "hello_world::Client",
            message: Some("expected UserData of type hello_world::Client".to_string()),
        })
    }
}


impl Client {
    pub async fn connect(addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let addr = addr.to_socket_addrs()?.next().expect("could not parse address");

        let stream = tokio::net::TcpStream::connect(&addr).await?;
        stream.set_nodelay(true)?;
        let (reader, writer) = tokio_util::compat::TokioAsyncReadCompatExt::compat(stream).split();
        let rpc_network = Box::new(twoparty::VatNetwork::new(
            futures::io::BufReader::new(reader),
            futures::io::BufWriter::new(writer),
            rpc_twoparty_capnp::Side::Client,
            Default::default(),
        ));
        let mut rpc_system = RpcSystem::new(rpc_network, None);
        let hello_front: hello_front::Client =
            rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);
        tokio::task::spawn_local(rpc_system);
        Ok(Client { hello_front })
    }

    pub async fn get_hello(&self) -> Result<hello_world::Client, Box<dyn std::error::Error>> {
        let mut request = self.hello_front.get_hello_request();
        let reply = request.send().promise.await?;
        let hello_cap = reply.get()?.get_reply();
        Ok(hello_cap?)
    }
}


#[derive(Debug, Clone)]
struct ComplexData {
    field1: String,
    field2: i32,
    field3: f64,
}

impl UserData for ComplexData {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("get_field1", |_, this, ()| {
            Ok(this.field1.clone())
        });
        methods.add_method("get_field2", |_, this, ()| {
            Ok(this.field2)
        });
        methods.add_method("get_field3", |_, this, ()| {
            Ok(this.field3)
        });
    }
}

fn rust_function() -> ComplexData {
    ComplexData {
        field1: "Hello".to_string(),
        field2: 123,
        field3: 45.67,
    }
}

pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = ::std::env::args().collect();
    if args.len() != 4 {
        println!("usage: {} client HOST:PORT MESSAGE", args[0]);
        return Ok(());
    }

    let addr = &args.clone()[2];

    tokio::task::LocalSet::new()
        .run_until(async move {
        let lua = Lua::new();

        let globals = lua.globals();
        let client = Rc::new(Client::connect(addr).await?);
        globals.set("rust_function", lua.create_function(|_, ()| {
            Ok(rust_function())
        })?)?;
        globals.set("get_hello", lua.create_async_function(move |_, ()| {
            let client = Rc::clone(&client);
            async move {
                let result = client.get_hello().await;
                match result {
                    Ok(response) => Ok(response),
                    Err(_) => Err(LuaError::external(eyre::eyre!("get_hello cap errored"))),
                }
            }
        })?)?;
        globals.set("say_hello", lua.create_async_function(move |_, lua_hello_client: hello_world::Client| {
            let lua_client = Rc::new(lua_hello_client);
            let rc_lua_client = Rc::clone(&lua_client);
            let args_clone = args.clone();
            async move {
                let msg = &args_clone[3];
                let mut request = rc_lua_client.say_hello_request();
                request.get().init_request().set_name(msg.as_str().into());

                let reply = request.send().promise.await.expect("Failed to send hello request");

                let message = reply.get().expect("Failed to get reply").get_reply().expect("Failed to get reply content").get_message().expect("Failed to get message").to_str().expect("Failed to convert message to str").to_string();
                Ok(message)
            }
        })?)?;
        let lua_result = lua.load(r#"
            local structured = rust_function()
            print("Received structured data from Rust function: ")
            print(structured:get_field1(), structured:get_field2(), structured:get_field3())
            local hello_cap = get_hello()
            print("Received hello cap from cap'n proto: ")
            print(hello_cap)
            local result = say_hello(hello_cap)
            print("Received hello from calling the cap: ")
            print(result)
        "#).exec_async().await;
        match lua_result {
            Ok(x) => println!("Ok: {:?}",x),
            Err(e) =>println!("Err: {:?}",e),
        }
        Ok(())
    })
    .await
}
