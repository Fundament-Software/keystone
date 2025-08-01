use crate::capnp::{self, dynamic_list, dynamic_value};
use crate::capnp::{any_pointer::Owned as any_pointer, dynamic_struct};
use crate::capnp_rpc::queued;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::{
    keystone::{Error, ModuleProgram},
    module_capnp::module_error,
    module_capnp::module_start,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, derive_more::TryFrom)]
#[try_from(repr)]
#[repr(u8)]
pub enum ModuleState {
    NotStarted,
    Initialized, // Started but waiting for bootstrap capability to return
    Ready,
    Paused,
    Closing, // Has been told to shut down but is saving it's state
    Closed,  // Clean shutdown, can be restarted safely
    Aborted, // Was abnormally terminated for some reason
    StartFailure,
    CloseFailure,
}

impl std::fmt::Display for ModuleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleState::NotStarted => write!(f, "Not Started"),
            ModuleState::Initialized => write!(f, "Initialized"),
            ModuleState::Ready => write!(f, "Ready"),
            ModuleState::Paused => write!(f, "Paused"),
            ModuleState::Closing => write!(f, "Closing"),
            ModuleState::Closed => write!(f, "Closed"),
            ModuleState::Aborted => write!(f, "Aborted"),
            ModuleState::StartFailure => write!(f, "Start Failure"),
            ModuleState::CloseFailure => write!(f, "Close Failure"),
        }
    }
}
#[derive(Clone)]
pub enum ModuleOrCap {
    InstanceId(u64),
    Cap(CapnpHook),
}

#[derive(Clone)]
pub struct CapnpHook {
    pub cap: Box<dyn capnp::private::capability::ClientHook>,
    pub instance_id: u64,
}

#[derive(Clone)]
pub struct ParamResultType {
    pub name: String,
    pub capnp_type: CapnpType,
}

impl TryInto<CapnpType> for dynamic_value::Reader<'_> {
    type Error = core::str::Utf8Error;

    fn try_into(self) -> std::result::Result<CapnpType, Self::Error> {
        Ok(match self {
            dynamic_value::Reader::Void => CapnpType::Void,
            dynamic_value::Reader::Bool(b) => CapnpType::Bool(Some(b)),
            dynamic_value::Reader::Int8(i) => CapnpType::Int8(Some(i)),
            dynamic_value::Reader::Int16(i) => CapnpType::Int16(Some(i)),
            dynamic_value::Reader::Int32(i) => CapnpType::Int32(Some(i)),
            dynamic_value::Reader::Int64(i) => CapnpType::Int64(Some(i)),
            dynamic_value::Reader::UInt8(u) => CapnpType::UInt8(Some(u)),
            dynamic_value::Reader::UInt16(u) => CapnpType::UInt16(Some(u)),
            dynamic_value::Reader::UInt32(u) => CapnpType::UInt32(Some(u)),
            dynamic_value::Reader::UInt64(u) => CapnpType::UInt64(Some(u)),
            dynamic_value::Reader::Float32(f) => CapnpType::Float32(Some(f)),
            dynamic_value::Reader::Float64(f) => CapnpType::Float64(Some(f)),
            dynamic_value::Reader::Enum(e) => CapnpType::Enum(CapnpEnum {
                value: Some(e.get_value()),
                schema: e.get_enumerant().unwrap().unwrap().get_containing_enum(),
                enumerant_name: Some(
                    e.get_enumerant()
                        .unwrap()
                        .unwrap()
                        .get_proto()
                        .get_name()
                        .unwrap()
                        .to_string()?,
                ),
            }),
            dynamic_value::Reader::Text(r) => CapnpType::Text(Some(r.to_string().unwrap())),
            dynamic_value::Reader::Data(d) => {
                let mut data = d.to_vec();
                data.insert(0, 0);
                CapnpType::Data(data)
            }
            dynamic_value::Reader::Struct(r) => struct_to_capnp_type(r)?,
            dynamic_value::Reader::List(r) => list_to_capnp_type(r)?,
            dynamic_value::Reader::AnyPointer(_) => CapnpType::AnyPointer(None), //TODO
            dynamic_value::Reader::Capability(cap) => CapnpType::Capability(CapnpCap {
                hook: None,
                schema: cap.get_schema(),
            }),
        })
    }
}
fn struct_to_capnp_type(r: dynamic_struct::Reader<'_>) -> Result<CapnpType, core::str::Utf8Error> {
    let schema = r.get_schema();
    let mut fields = Vec::new();
    fields.push(ParamResultType {
        name: "".to_string(),
        capnp_type: CapnpType::None,
    });
    for field in schema.get_fields().unwrap() {
        fields.push(ParamResultType {
            name: field.get_proto().get_name().unwrap().to_string().unwrap(),
            capnp_type: match r.get(field).unwrap() {
                dynamic_value::Reader::Void => CapnpType::Void,
                dynamic_value::Reader::Bool(b) => CapnpType::Bool(Some(b)),
                dynamic_value::Reader::Int8(i) => CapnpType::Int8(Some(i)),
                dynamic_value::Reader::Int16(i) => CapnpType::Int16(Some(i)),
                dynamic_value::Reader::Int32(i) => CapnpType::Int32(Some(i)),
                dynamic_value::Reader::Int64(i) => CapnpType::Int64(Some(i)),
                dynamic_value::Reader::UInt8(u) => CapnpType::UInt8(Some(u)),
                dynamic_value::Reader::UInt16(u) => CapnpType::UInt16(Some(u)),
                dynamic_value::Reader::UInt32(u) => CapnpType::UInt32(Some(u)),
                dynamic_value::Reader::UInt64(u) => CapnpType::UInt64(Some(u)),
                dynamic_value::Reader::Float32(f) => CapnpType::Float32(Some(f)),
                dynamic_value::Reader::Float64(f) => CapnpType::Float64(Some(f)),
                dynamic_value::Reader::Enum(e) => CapnpType::Enum(CapnpEnum {
                    value: Some(e.get_value()),
                    schema: e.get_enumerant().unwrap().unwrap().get_containing_enum(),
                    enumerant_name: Some(
                        e.get_enumerant()
                            .unwrap()
                            .unwrap()
                            .get_proto()
                            .get_name()
                            .unwrap()
                            .to_string()?,
                    ),
                }),
                dynamic_value::Reader::Text(reader) => CapnpType::Text(Some(reader.to_string()?)),
                dynamic_value::Reader::Data(items) => CapnpType::Data(items.to_vec()),
                dynamic_value::Reader::Struct(reader) => struct_to_capnp_type(reader)?,
                dynamic_value::Reader::List(reader) => list_to_capnp_type(reader)?,
                dynamic_value::Reader::AnyPointer(_) => CapnpType::Void, //TODO
                dynamic_value::Reader::Capability(cap) => CapnpType::Capability(CapnpCap {
                    hook: None, //TODO set this while getting struct
                    schema: cap.get_schema(),
                }),
            },
        });
    }
    Ok(CapnpType::Struct(CapnpStruct {
        fields,
        schema: r.get_schema(),
    }))
}
fn list_to_capnp_type(r: dynamic_list::Reader<'_>) -> Result<CapnpType, core::str::Utf8Error> {
    let mut items = Vec::new();
    items.push(CapnpType::None);
    for item in r.iter() {
        items.push(match item.unwrap() {
            dynamic_value::Reader::Void => CapnpType::Void,
            dynamic_value::Reader::Bool(b) => CapnpType::Bool(Some(b)),
            dynamic_value::Reader::Int8(i) => CapnpType::Int8(Some(i)),
            dynamic_value::Reader::Int16(i) => CapnpType::Int16(Some(i)),
            dynamic_value::Reader::Int32(i) => CapnpType::Int32(Some(i)),
            dynamic_value::Reader::Int64(i) => CapnpType::Int64(Some(i)),
            dynamic_value::Reader::UInt8(u) => CapnpType::UInt8(Some(u)),
            dynamic_value::Reader::UInt16(u) => CapnpType::UInt16(Some(u)),
            dynamic_value::Reader::UInt32(u) => CapnpType::UInt32(Some(u)),
            dynamic_value::Reader::UInt64(u) => CapnpType::UInt64(Some(u)),
            dynamic_value::Reader::Float32(f) => CapnpType::Float32(Some(f)),
            dynamic_value::Reader::Float64(f) => CapnpType::Float64(Some(f)),
            dynamic_value::Reader::Enum(e) => CapnpType::Enum(CapnpEnum {
                value: Some(e.get_value()),
                schema: e.get_enumerant().unwrap().unwrap().get_containing_enum(),
                enumerant_name: Some(
                    e.get_enumerant()
                        .unwrap()
                        .unwrap()
                        .get_proto()
                        .get_name()
                        .unwrap()
                        .to_string()?,
                ),
            }),
            dynamic_value::Reader::Text(reader) => CapnpType::Text(Some(reader.to_string()?)),
            dynamic_value::Reader::Data(items) => CapnpType::Data(items.to_vec()),
            dynamic_value::Reader::Struct(reader) => struct_to_capnp_type(reader)?,
            dynamic_value::Reader::List(reader) => list_to_capnp_type(reader)?,
            dynamic_value::Reader::AnyPointer(_) => CapnpType::Void, //TODO
            dynamic_value::Reader::Capability(cap) => CapnpType::Capability(CapnpCap {
                hook: None, //TODO set this while getting struct
                schema: cap.get_schema(),
            }),
        });
    }
    Ok(CapnpType::List(items))
}
#[derive(Clone)]
pub enum CapnpType {
    Void,
    Bool(Option<bool>),
    Int8(Option<i8>),
    Int16(Option<i16>),
    Int32(Option<i32>),
    Int64(Option<i64>),
    UInt8(Option<u8>),
    UInt16(Option<u16>),
    UInt32(Option<u32>),
    UInt64(Option<u64>),
    Float32(Option<f32>),
    Float64(Option<f64>),
    Enum(CapnpEnum),
    Text(Option<String>),
    Data(Vec<u8>),
    Struct(CapnpStruct),
    List(Vec<CapnpType>),
    AnyPointer(Option<Box<CapnpType>>), //TODO requires some ui elemnts to make sense
    Capability(CapnpCap),
    None,
}

#[derive(Clone)]
pub struct CapnpCap {
    pub hook: Option<Box<dyn capnp::private::capability::ClientHook>>,
    pub schema: capnp::schema::CapabilitySchema,
}

#[derive(Clone)]
pub struct CapnpEnum {
    pub value: Option<u16>,
    pub schema: capnp::schema::EnumSchema,
    pub enumerant_name: Option<String>,
}

#[derive(Clone)]
pub struct CapnpStruct {
    pub fields: Vec<ParamResultType>,
    pub schema: capnp::schema::StructSchema,
}

impl CapnpStruct {
    pub fn init(&mut self) {
        if self.fields.len() == 1 {
            for field in self.schema.get_fields().unwrap() {
                self.fields.push(ParamResultType {
                    name: field.get_proto().get_name().unwrap().to_string().unwrap(),
                    capnp_type: field.get_type().which().into(),
                });
            }
        }
    }
}
impl From<capnp::introspect::TypeVariant> for CapnpType {
    fn from(val: capnp::introspect::TypeVariant) -> Self {
        match val {
            capnp::introspect::TypeVariant::Void => CapnpType::Void,
            capnp::introspect::TypeVariant::Bool => CapnpType::Bool(None),
            capnp::introspect::TypeVariant::Int8 => CapnpType::Int8(None),
            capnp::introspect::TypeVariant::Int16 => CapnpType::Int16(None),
            capnp::introspect::TypeVariant::Int32 => CapnpType::Int32(None),
            capnp::introspect::TypeVariant::Int64 => CapnpType::Int64(None),
            capnp::introspect::TypeVariant::UInt8 => CapnpType::UInt8(None),
            capnp::introspect::TypeVariant::UInt16 => CapnpType::UInt16(None),
            capnp::introspect::TypeVariant::UInt32 => CapnpType::UInt32(None),
            capnp::introspect::TypeVariant::UInt64 => CapnpType::UInt64(None),
            capnp::introspect::TypeVariant::Float32 => CapnpType::Float32(None),
            capnp::introspect::TypeVariant::Float64 => CapnpType::Float64(None),
            capnp::introspect::TypeVariant::Text => CapnpType::Text(None),
            capnp::introspect::TypeVariant::Data => CapnpType::Data(vec![0]),
            capnp::introspect::TypeVariant::Struct(raw_branded_struct_schema) => {
                CapnpType::Struct(CapnpStruct {
                    fields: vec![ParamResultType {
                        name: "".to_string(),
                        capnp_type: CapnpType::None,
                    }],
                    schema: raw_branded_struct_schema.into(),
                })
            }
            capnp::introspect::TypeVariant::AnyPointer => CapnpType::AnyPointer(None),
            capnp::introspect::TypeVariant::Capability(raw_capability_schema) => {
                CapnpType::Capability(CapnpCap {
                    hook: None,
                    schema: raw_capability_schema.into(),
                })
            }
            capnp::introspect::TypeVariant::Enum(raw_enum_schema) => CapnpType::Enum(CapnpEnum {
                value: None,
                schema: raw_enum_schema.into(),
                enumerant_name: None,
            }),
            capnp::introspect::TypeVariant::List(ty) => CapnpType::List(vec![ty.which().into()]),
        }
    }
}

impl std::fmt::Display for ParamResultType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.capnp_type.to_string(self.name.as_str()))
    }
}

macro_rules! format_capnp_type {
    ($name:expr, $value:expr, $type_name:literal) => {{
        if let Some(val) = $value {
            format!("{} - {} :{}", $name, val, $type_name)
        } else {
            format!("{} :{}", $name, $type_name)
        }
    }};
}

impl CapnpType {
    pub fn to_string(&self, name: &str) -> String {
        match self {
            CapnpType::Void => format!("{name} :Void,"),
            CapnpType::Bool(b) => format_capnp_type!(name, b, "Bool"),
            CapnpType::Int8(i) => format_capnp_type!(name, i, "Int8"),
            CapnpType::Int16(i) => format_capnp_type!(name, i, "Int16"),
            CapnpType::Int32(i) => format_capnp_type!(name, i, "Int32"),
            CapnpType::Int64(i) => format_capnp_type!(name, i, "Int64"),
            CapnpType::UInt8(u) => format_capnp_type!(name, u, "UInt8"),
            CapnpType::UInt16(u) => format_capnp_type!(name, u, "UInt16"),
            CapnpType::UInt32(u) => format_capnp_type!(name, u, "UInt32"),
            CapnpType::UInt64(u) => format_capnp_type!(name, u, "UInt64"),
            CapnpType::Float32(f) => format_capnp_type!(name, f, "Float32"),
            CapnpType::Float64(f) => format_capnp_type!(name, f, "Float64"),
            CapnpType::Enum(e) => format_capnp_type!(name, e.enumerant_name.as_ref(), "Enum"),
            CapnpType::Text(t) => format_capnp_type!(name, t, "Text"),
            CapnpType::AnyPointer(capnp_type) => {
                if let Some(c) = capnp_type {
                    format!("{name} - {} :AnyPointer", c.to_string(""))
                } else {
                    format!("{name} :AnyPointer")
                }
            }
            CapnpType::Data(items) => {
                let mut fields = Vec::new();
                let mut iter = items.iter();
                iter.next();
                for v in iter {
                    fields.push(v.to_string());
                }
                format!("{name} - [{}] :Data", fields.join(", "))
            }
            CapnpType::Struct(st) => {
                let mut fields = Vec::new();
                let mut iter = st.fields.iter();
                iter.next();
                for v in iter {
                    fields.push(v.to_string());
                }
                format!("{{{}}} :{name}", fields.join(", "))
            }
            CapnpType::List(l) => {
                let mut fields = Vec::new();
                let mut iter = l.iter();
                iter.next();
                for v in iter {
                    fields.push(v.to_string(""));
                }
                format!("{name} - {{{}}} :List<>", fields.join(", ")) //TODO list type
            }
            CapnpType::Capability(c) => {
                //TODO specify cap
                let ty = if let Ok(r) = &c.schema.get_proto().get_display_name() {
                    &r.to_str().unwrap()
                        [c.schema.get_proto().get_display_name_prefix_length() as usize..]
                } else {
                    "Client"
                };
                if let Some(c) = &c.hook {
                    format!("{name} - Some({}) :{ty}", c.get_brand())
                } else {
                    format!("{name} :{ty}")
                }
            }
            CapnpType::None => name.to_string(),
        }
    }
}
pub struct FunctionDescription {
    pub module_or_cap: ModuleOrCap,
    pub function_name: String,
    pub type_id: u64,
    pub method_id: u16,
    pub params: Vec<ParamResultType>,
    pub params_schema: u64,
    pub results: Vec<ParamResultType>,
    pub results_schema: u64,
}
//TODO potentially doesn't work for multiple of the same module
impl std::hash::Hash for FunctionDescription {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if let ModuleOrCap::InstanceId(id) = self.module_or_cap {
            id.hash(state);
        }
        self.function_name.hash(state);
        self.type_id.hash(state);
    }
}
impl PartialEq for FunctionDescription {
    fn eq(&self, other: &Self) -> bool {
        self.function_name == other.function_name && self.type_id == other.type_id
    }
}
impl Eq for FunctionDescription {}

// Defines something that can spawn a ModuleInstance, which is usually a ModuleProgram
pub struct ModuleSource {
    pub schema_id: u64,
    pub(crate) program: ModuleProgram,
    pub dyn_schema: capnp::schema::DynamicSchema,
}

// Defines a particular module instance being tracked by Keystone
pub struct ModuleInstance {
    //pub id: spawn_capnp::identifier::Client,
    pub source: std::path::PathBuf,
    //pub instance_id: u64,
    pub name: String, // Simply stores id.to_string() so we don't need access to the id_set.
    // A module isn't always a process, but if it does have a process we need the ability to kill it
    pub(crate) process: Option<crate::ModuleProcess>,
    pub(crate) bootstrap: Option<module_start::Client<any_pointer, any_pointer>>,
    pub pause: tokio::sync::watch::Sender<bool>,
    pub state: std::sync::atomic::AtomicU8,
    pub api: queued::Client,
}

impl ModuleInstance {
    fn check_error(
        name: &str,
        result: Result<
            capnp::capability::Response<
                crate::spawn_capnp::process::join_results::Owned<
                    any_pointer,
                    module_error::Owned<any_pointer>,
                >,
            >,
            capnp::Error,
        >,
    ) -> Result<ModuleState> {
        let r = match result {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("{} process returned error: {}", name, e.to_string());
                return Err(e.into());
            }
        };

        let moderr: module_error::Reader<any_pointer> = r.get()?.get_result()?;
        Ok(match moderr.which()? {
            module_error::Which::Backing(e) => {
                let e: crate::posix_capnp::posix_error::Reader = e?.get_as()?;
                if e.get_error_code() != 0 {
                    tracing::error!(
                        "{} process returned error code: {}",
                        name,
                        e.get_error_code()
                    );
                    ModuleState::CloseFailure
                } else {
                    ModuleState::Closed
                }
            }
            _ => ModuleState::CloseFailure,
        })
    }

    fn reset(&mut self) {
        self.process = None;
        self.bootstrap = None;
        self.api = queued::Client::new(None);
        self.pause.send_replace(false);
    }

    pub async fn stop(&mut self, timeout: Duration) -> Result<()> {
        if self.halted() {
            return Ok(());
        }
        self.pause.send_replace(false);
        self.state
            .store(ModuleState::Closing as u8, Ordering::Release);

        // Send the stop request to the bootstrap interface
        let stop_request = {
            let Some(bootstrap) = self.bootstrap.as_ref() else {
                return Err(Error::MissingBootstrap(self.name.clone()).into());
            };

            bootstrap.stop_request().send()
        };

        tracing::debug!("Sent a stop_request to {}", &self.name);

        // Call the stop method with some timeout
        if (tokio::time::timeout(timeout, stop_request.promise).await).is_err() {
            // Force kill the module.
            tracing::warn!("{} timed out when requesting stop!", &self.name);
            self.kill().await;
            Ok(())
        } else {
            tracing::debug!("Got stop_request reply back from {}", &self.name);

            if let Some(p) = self.process.as_ref() {
                // Now join the process with the same timeout
                match tokio::time::timeout(timeout, p.join_request().send().promise).await {
                    Ok(result) => {
                        tracing::debug!("Joined process for {}", &self.name);
                        let end_state = match Self::check_error(&self.name, result) {
                            Ok(v) => v as u8,
                            Err(e) => {
                                tracing::error!(
                                    "Failure during {} error lookup: {}",
                                    &self.name,
                                    e.to_string()
                                );
                                ModuleState::CloseFailure as u8
                            }
                        };

                        self.reset(); // Always reset BEFORE setting the state to prevent race conditions
                        self.state.store(end_state, Ordering::Release)
                    }
                    Err(_) => self.kill().await,
                }
            } else {
                self.reset();
                self.state
                    .store(ModuleState::Closed as u8, Ordering::Release);
            }

            Ok(())
        }
    }

    pub async fn kill(&mut self) {
        if let Some(p) = self.process.as_ref() {
            tracing::warn!("Force killing {}", &self.name);
            let _ = p.kill_request().send().promise.await;
        } else {
            panic!("Tried to kill a module that doesn't have a process!")
        }

        self.reset();
        self.state
            .store(ModuleState::Aborted as u8, Ordering::Release);
    }

    pub async fn pause(&mut self, pause: bool) -> Result<()> {
        // Only change pause state if we're in a Ready or Paused state.
        if pause && self.state.load(Ordering::Acquire) == ModuleState::Ready as u8 {
            self.pause.send_replace(true);
            self.state
                .store(ModuleState::Paused as u8, Ordering::Release);
        } else if !pause && self.state.load(Ordering::Acquire) == ModuleState::Paused as u8 {
            self.pause.send_replace(false);
            self.state
                .store(ModuleState::Ready as u8, Ordering::Release);
        }
        Ok(())
    }

    pub(crate) fn halted(&self) -> bool {
        let v = self.state.load(Ordering::Acquire);

        v == ModuleState::NotStarted as u8
            || v == ModuleState::Closed as u8
            || v == ModuleState::Aborted as u8
            || v == ModuleState::StartFailure as u8
            || v == ModuleState::CloseFailure as u8
    }
}
