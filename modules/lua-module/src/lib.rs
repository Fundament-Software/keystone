use capnp::schema::CapabilitySchema;
use keystone::proxy::GetPointerReader;
use mlua::prelude::*;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use capnp::capability::FromClientHook;
use capnp::private::capability::ClientHook;
use capnp::{any_pointer, dynamic_struct, dynamic_value};
use capnp_macros::capnproto_rpc;
use crate::spawn_capnp::program::SpawnParams;
use crate::lua_module_capnp::{lua_module_api, hello_world};
use crate::spawn_capnp::{program, process};
use capnp::traits::{HasTypeId, IntoInternalStructReader};
use crate::module_capnp::module_error;
use capnp::traits::FromPointerReader;


capnp_import::capnp_import!("../../core/schema/storage.capnp", "../../core/schema/module.capnp", "../../core/schema/spawn.capnp", "lua_module.capnp");

struct LuaProgramImpl {
}
struct LuaProcessImpl {
    api: Rc<LuaProcessApiImpl>
}
#[capnproto_rpc(process)]
impl <Er: capnp::traits::Owned>process::Server<lua_module_api::Owned, Er> for LuaProcessImpl {
    async fn get_api(&self) -> Result<(), capnp::Error> {

        results.get().set_api(capnp_rpc::new_client(self.api.clone()))?;
        Ok(())
    }
}

struct LuaProcessApiImpl {
    methods: Vec<String>,
    call: Vec<Box<dyn Fn() -> capnp::capability::Request<any_pointer::Owned, any_pointer::Owned>>>
}
impl LuaProcessApiImpl {
    fn new() -> Self {
        Self { methods: Vec::new(), call: Vec::new() }
    }
}
#[capnproto_rpc(lua_module_api)]
impl lua_module_api::Server for Rc<LuaProcessApiImpl> {
    async fn get_methods(&self) {
        let mut builder = results.get().init_method_names(self.methods.len() as u32);
        for (index, name) in self.methods.iter().enumerate() {
            builder.set(index as u32, name.as_str().into());
        }
        Ok(())
    }
    async fn call_method(&self, id: u8) {
        //TODO params, results
        let response = self.call[id as usize]().send().promise.await?;
        //let result: dynamic_struct::Reader<'_> = response.get()?;
        Ok(())
    }
}
impl lua_module_api::Client {
    
}
#[capnproto_rpc(program)]
impl <C: capnp::traits::Owned> program::Server<C, lua_module_api::Owned, module_error::Owned<capnp::any_pointer::Owned>
> for LuaProgramImpl where for<'a> C::Reader<'a>: capnp::capability::FromClientHook + capnp::traits::HasTypeId {
    async fn spawn(&self,
        args: SpawnParams
    ) -> Result<(), ::capnp::Error> {
        let mut process_api = LuaProcessApiImpl::new();
        let hook = args.into_client_hook();
        let id = C::Reader::TYPE_ID;
        match C::introspect().which() {
            capnp::introspect::TypeVariant::Capability(schema) => {
                let schema: capnp::schema::CapabilitySchema = schema.into();
                match schema.get_proto().which()? {
                    capnp::schema_capnp::node::Which::Interface(interface) => {
                        let methods = interface.get_methods()?;
                        for (ordinal, method) in methods.into_iter().enumerate() {
                            let name = method.get_name()?.to_string()?;
                            process_api.methods.push(name);
                            let cloned_hook = hook.add_ref();
                            process_api.call.push(Box::new(move || {
                                cloned_hook.new_call(id, ordinal as u16, None)
                            }));
                        }
                    },
                    _ => return Err(capnp::Error::failed("Non capability provided as argument to lua module spawn".to_string())),
                }
            },
            _ => return Err(capnp::Error::failed("Non capability provided as argument to lua module spawn".to_string())),
        }

        results.get().set_result(capnp_rpc::new_client(LuaProcessImpl{api: Rc::new(process_api)}));
        Ok(())
    }

}
struct FakeKeystoneImpl {
    hello_world_cap: hello_world::Client
}
#[capnproto_rpc(lua_module_capnp::fake_keystone)]
impl lua_module_capnp::fake_keystone::Server for FakeKeystoneImpl {
    async fn get_lua_hello_world_module(&self) {
        results.get().set_hello(self.hello_world_cap.clone());
        Ok(())
    }
}
struct TestHelloWorld {

}
#[capnproto_rpc(hello_world)]
impl hello_world::Server for TestHelloWorld {
    async fn hi(&self, number: u8) {
        println!("Hello world!");
        results.get().set_test(number);
        Ok(())
    }
}
#[derive(Clone, FromLua)]
struct dyn_struct<'a> {
    d: capnp::dynamic_struct::Reader<'a>
}
#[derive(Clone, FromLua)]
struct LuaCap<C: capnp::capability::FromClientHook> {
    cap: C
}
impl <C: capnp::capability::FromClientHook>mlua::UserData for LuaCap<C> {

}
#[derive(Clone, FromLua)]
struct Hmm {
    cap: Box<dyn ClientHook>
}
async fn wrap_lua_cap<C: capnp::capability::FromClientHook>(lua: &Lua, lua_cap: LuaCap<C>) -> LuaResult<LuaTable> {
    let functions = lua.create_table()?;
    match C::introspect().which() {
        capnp::introspect::TypeVariant::Capability(raw_schema) => {
            read_cap_schema(lua, raw_schema.into(), &functions, lua_cap.cap.into_client_hook())?;
        },
        _ => return Err(mlua::Error::runtime("Non capability provided as argument to wrap lua cap".to_string())),
    }
    Ok(functions)
}
fn read_dyn_struct(lua: &Lua, dyn_reader: dynamic_struct::Reader, results_table: &LuaTable) -> LuaResult<()> {
    let fields = dyn_reader.get_schema().get_fields().unwrap();
    let mut pointer_index = 0;
    for field in fields {
        let field_name = field.get_proto().get_name().unwrap().to_str()?;
        match dyn_reader.get(field).unwrap() {
            dynamic_value::Reader::Void => {
                results_table.set(field_name, LuaNil)?;
            },
            dynamic_value::Reader::Bool(b) => {
                results_table.set(field_name, b)?;
            },
            dynamic_value::Reader::Int8(i) => {
                results_table.set(field_name, i)?;
            },
            dynamic_value::Reader::Int16(i) => {
                results_table.set(field_name, i)?;
            },
            dynamic_value::Reader::Int32(i) => {
                results_table.set(field_name, i)?;
            },
            dynamic_value::Reader::Int64(i) => {
                results_table.set(field_name, i)?;
            },
            dynamic_value::Reader::UInt8(u) => {
                results_table.set(field_name, u)?;
            },
            dynamic_value::Reader::UInt16(u) => {
                results_table.set(field_name, u)?;
            },
            dynamic_value::Reader::UInt32(u) => {
                results_table.set(field_name, u)?;
            },
            dynamic_value::Reader::UInt64(u) => {
                results_table.set(field_name, u)?;
            },
            dynamic_value::Reader::Float32(f) => {
                results_table.set(field_name, f)?;
            },
            dynamic_value::Reader::Float64(f) => {
                results_table.set(field_name, f)?;
            },
            dynamic_value::Reader::Enum(_) => todo!(),
            dynamic_value::Reader::Text(r) =>  {
                results_table.set(field_name, r.to_str()?)?;
            },
            dynamic_value::Reader::Data(d) =>  {
                results_table.set(field_name, d)?;
            },
            dynamic_value::Reader::Struct(r) => {
                let inner_struct_table = lua.create_table()?;
                read_dyn_struct(lua, r, &inner_struct_table)?;
                results_table.set(field_name, inner_struct_table)?;
            },
            dynamic_value::Reader::List(_) => todo!(),
            dynamic_value::Reader::AnyPointer(_) => todo!(),
            dynamic_value::Reader::Capability(cap) => {
                //TODO save the Box<dyn ClientHook> as well
                let inner_cap_table = lua.create_table()?;
                //inner_cap_table.set("cap", Box<dyn Clienthook>)
                //TODO get actual offset for the correct capability field somewhere or just count manually
                //let hook = dyn_reader.reader.get_pointer_field(pointer_index).get_capability().unwrap();
                let hook = dyn_reader.get_clienthook(field).unwrap();
                pointer_index += 1;
                read_cap_schema(lua, cap.get_schema(), &inner_cap_table, hook)?;
                results_table.set(field_name, inner_cap_table)?;
            },
        }
    }
    Ok(())
}
fn read_cap_schema(lua: &Lua, schema: CapabilitySchema, functions: &LuaTable, hook: Box<dyn ClientHook>) -> LuaResult<()> {
            let id = schema.get_proto().get_id();
            //schema.get_proto().get_nested_nodes().unwrap();;
            match schema.get_proto().which().unwrap() {
                capnp::schema_capnp::node::Which::Interface(interface) => {
                    let methods = interface.get_methods().unwrap();
                    for (ordinal, method) in methods.into_iter().enumerate() {
                            let cloned_hook = hook.add_ref();
                            functions.set(method.get_name().unwrap().to_str()?, lua.create_async_function(move |lua: &Lua, params: Option<LuaTable>| {
                            let value = cloned_hook.clone();
                            async move {
                                let results_table = lua.create_table()?;
                                let mut call = value.new_call(id, ordinal as u16, None);
                                if let Some(params) = params {
                                    let mut dyn_struct_builder = call.get().init_dynamic(schema.get_params_struct_schema(ordinal as u16).unwrap().into()).unwrap();
                                    for pair in params.pairs::<mlua::Value, mlua::Value>() {
                                        //TODO other types
                                        let pair = pair?;
                                        match pair.0 {
                                            LuaValue::String(name) => {
                                                match pair.1 {
                                                    LuaNil => {
                                                        dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::Void).unwrap();
                                                    },
                                                    LuaValue::Boolean(b) => {
                                                        dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::Bool(b)).unwrap();
                                                    },
                                                    LuaValue::LightUserData(_) => todo!(),
                                                    LuaValue::Integer(i) => {
                                                        match dyn_struct_builder.reborrow().get_named(name.to_str()?).unwrap() {
                                                            dynamic_value::Builder::Int8(_) => {
                                                                dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::Int8(i.try_into().unwrap())).unwrap();
                                                            },
                                                            dynamic_value::Builder::Int16(_) => {
                                                                dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::Int16(i.try_into().unwrap())).unwrap();
                                                            },
                                                            dynamic_value::Builder::Int32(_) => {
                                                                dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::Int32(i.try_into().unwrap())).unwrap();
                                                            },
                                                            dynamic_value::Builder::Int64(_) => {
                                                                dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::Int64(i)).unwrap();
                                                            },
                                                            dynamic_value::Builder::UInt8(_) =>  {
                                                                dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::UInt8(i.try_into().unwrap())).unwrap();
                                                            },
                                                            dynamic_value::Builder::UInt16(_) =>  {
                                                                dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::UInt16(i.try_into().unwrap())).unwrap();
                                                            },
                                                            dynamic_value::Builder::UInt32(_) =>  {
                                                                dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::UInt32(i.try_into().unwrap())).unwrap();
                                                            },
                                                            dynamic_value::Builder::UInt64(_) =>  {
                                                                dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::UInt64(i.try_into().unwrap())).unwrap();
                                                            },
                                                            _ => {
                                                                return Err(mlua::Error::runtime(format!("TODO")))
                                                            },
                                                        };
                                                    },
                                                    LuaValue::Number(n) =>  {
                                                        match dyn_struct_builder.reborrow().get_named(name.to_str()?).unwrap() {
                                                            dynamic_value::Builder::Float32(_) => {
                                                                dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::Float32(n as f32)).unwrap();
                                                                //TODO this conversion is undefined behavior for floats that are too big I think
                                                            },
                                                            dynamic_value::Builder::Float64(_) => {
                                                                dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::Float64(n)).unwrap();
                                                            },
                                                            _ => {
                                                                return Err(mlua::Error::runtime(format!("TODO")))
                                                            },
                                                        };
                                                    },
                                                    LuaValue::String(s) => {
                                                        dyn_struct_builder.set_named(name.to_str()?, dynamic_value::Reader::Text(s.to_str()?.into())).unwrap();
                                                    },
                                                    LuaValue::Table(_) => todo!(),
                                                    LuaValue::Function(_) => todo!(),
                                                    LuaValue::Thread(_) => todo!(),
                                                    LuaValue::UserData(_) => todo!(),
                                                    LuaValue::Error(_) => todo!(),
                                                }
                                            },
                                            _ => return Err(mlua::Error::runtime("Non string used as key when passing params to a capability method"))
                                        }
                                    }
                                }
                                let response = call.send().promise.await.unwrap();
                                let get: keystone::proxy::GetPointerReader = response.get().unwrap().get_as().unwrap();
                                let res_struct = get.reader.get_struct(None).unwrap();
                                let res_schema = schema.get_results_struct_schema(ordinal as u16).unwrap();
                                let dyn_reader = dynamic_struct::Reader::new(res_struct, res_schema.into());
                                read_dyn_struct(lua, dyn_reader, &results_table)?;
                                Ok(results_table)
                            }
                            })?)?;
                    }
                },
                _ => return Err(mlua::Error::runtime("Non capability provided as argument to wrap lua cap".to_string())),
            }
            Ok(())
}
/*#[mlua::lua_module]
fn lua_bootstrap(lua: &Lua) -> LuaResult<LuaTable> {
    let functions = lua.create_table()?;
    
    functions.set("bootstrap", lua.create_async_function(bootstrap)?)?;
    Ok(functions)
}*/
/* 
*/
async fn register_functions(lua: &Lua, cap: lua_module_api::Client) -> LuaResult<LuaTable> {
    let functions = lua.create_table()?;
    let method_names_result = cap.get_methods_request().send().promise.await.unwrap();
    let method_names = method_names_result.get().unwrap().get_method_names().unwrap();
    for (id, name) in method_names.iter().enumerate() {
        let cloned_cap = cap.clone();
        functions.set(name.unwrap().to_str().unwrap(), lua.create_async_function(move |lua: &Lua, _: ()| {
        let value = cloned_cap.clone();
        async move {
            value.build_call_method_request(id as u8).send().promise.await.unwrap();
            Ok(())
        }
        })?)?;
    }

    Ok(functions)
}
async fn bootstrap(lua: &Lua, _: ()) -> LuaResult<LuaTable> {
    //TODO actually bootstrap keystone/Maybe add
    let keystone_cap: lua_module_capnp::fake_keystone::Client = capnp_rpc::new_client(FakeKeystoneImpl{hello_world_cap: capnp_rpc::new_client(TestHelloWorld{})});
    let program_client: program::Client<lua_module_capnp::fake_keystone::Owned, lua_module_api::Owned, crate::module_capnp::module_error::Owned<capnp::any_pointer::Owned>>
            = capnp_rpc::new_client(LuaProgramImpl{});
    let mut request = program_client.spawn_request();
    request.get().set_args(keystone_cap).unwrap();
    let process = request.send().promise.await.unwrap().get().unwrap().get_result().unwrap();
    //TODO hold on to the process cap
    let api = process.get_api_request().send().promise.await.unwrap().get().unwrap().get_api().unwrap();
    let functions = register_functions(lua, api).await?;
    //functions.set("wrap_capability", lua.create_async_function(spawn_lua_module)?)?;
    Ok(functions)
}
async fn test_bootstrap(lua: &Lua, _:()) -> LuaResult<LuaTable> {
    let client: lua_module_capnp::fake_keystone::Client = capnp_rpc::new_client(FakeKeystoneImpl{hello_world_cap: capnp_rpc::new_client(TestHelloWorld{})});
    let lua_cap = LuaCap{cap: client};
    let functions = wrap_lua_cap(lua, lua_cap).await?;
    Ok(functions)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test()]
    async fn test() -> eyre::Result<()> {
        let program_client: 
            program::Client<hello_world::Owned, lua_module_api::Owned, crate::module_capnp::module_error::Owned<capnp::any_pointer::Owned>>
            = capnp_rpc::new_client(LuaProgramImpl{});
        let hello_world_client: hello_world::Client = capnp_rpc::new_client(TestHelloWorld{});
        let mut request = program_client.spawn_request();
        request.get().set_args(hello_world_client)?;
        let process = request.send().promise.await?.get()?.get_result()?;
        let api = process.get_api_request().send().promise.await?.get()?.get_api()?;
        let method_names_result = api.get_methods_request().send().promise.await?;
        let method_names = method_names_result.get()?.get_method_names()?;
        for (id, name) in method_names.iter().enumerate() {
            println!("Method {id} = {}", name?.to_str()?);
            api.build_call_method_request(id as u8).send().promise.await?;
        }

        Ok(())
    }
    #[tokio::test()]
    async fn lua_test() -> eyre::Result<()> {
        let lua = Lua::new();
        //lua.globals().set("wrap_lua_cap", lua.create_async_function(wrap_lua_cap::<>)?)?;
        //local fake_keystone = require("lua_bootstrap")
        lua.globals().set("bootstrap", lua.create_async_function(test_bootstrap)?)?;
        lua.load(r#"
            local fake_keystone = bootstrap()
            local results = fake_keystone.getLuaHelloWorldModule()
            local hi_results = results.hello.hi({number=8})
            print(hi_results.test)
        "#).exec_async().await?;
        Ok(())
    }
}