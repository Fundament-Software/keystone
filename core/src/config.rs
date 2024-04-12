use std::any::Any;

use crate::toml_capnp;
use capnp::{
    dynamic_struct, dynamic_value,
    introspect::{Introspect, TypeVariant},
    schema_capnp,
    traits::HasTypeId,
};
use eyre::{eyre, Result};
use toml::{value::Offset, Table, Value};

fn value_to_list(l: &Vec<Value>, mut builder: ::capnp::dynamic_list::Builder) -> Result<()> {
    'outer: for idx in 0..builder.len() {
        let mut builder = builder.reborrow();

        match &l[idx as usize] {
            Value::String(s) => {
                if let TypeVariant::Enum(x) = builder.element_type().which() {
                    let concrete: capnp::schema::EnumSchema = x.into();
                    for e in concrete.get_enumerants()? {
                        if s.eq_ignore_ascii_case(e.get_proto().get_name()?.to_str()?) {
                            builder.set(
                                idx,
                                dynamic_value::Enum::new(e.get_ordinal(), concrete).into(),
                            )?;
                            continue 'outer;
                        }
                    }
                    return Err(eyre!("{:?} is not a valid enumeration value!", s));
                } else {
                    builder.set(idx, s.as_str().into())?
                }
            }
            Value::Integer(i) => builder.set(idx, (*i).into())?,
            Value::Float(f) => builder.set(idx, (*f).into())?,
            Value::Boolean(b) => builder.set(idx, (*b).into())?,
            Value::Datetime(d) => builder.set(idx, d.to_string().as_str().into())?,
            Value::Array(l) => value_to_list(l, builder.init(idx, l.len() as u32)?.downcast())?,
            Value::Table(t) => value_to_struct(t, builder.get(idx)?.downcast())?,
        }
    }
    Ok(())
}

fn toml_to_capnp(v: &Value, mut builder: toml_capnp::value::Builder) -> Result<()> {
    match v {
        Value::String(s) => builder.set_string(s.as_str().into()),
        Value::Integer(i) => builder.set_int((*i).into()),
        Value::Float(f) => builder.set_float((*f).into()),
        Value::Boolean(b) => builder.set_boolean((*b).into()),
        Value::Datetime(d) => {
            let mut dt = builder.init_datetime();
            if let Some(x) = d.date {
                dt.set_year(x.year);
                dt.set_month(x.month);
                dt.set_day(x.day);
            }
            if let Some(x) = d.time {
                dt.set_hour(x.hour);
                dt.set_minute(x.minute);
                dt.set_second(x.second);
                dt.set_nano(x.nanosecond);
            }
            match d.offset {
                Some(Offset::Z) => dt.set_offset(0),
                Some(Offset::Custom { minutes }) => dt.set_offset(minutes),
                None => (),
            }
        }
        Value::Array(l) => {
            let mut b = builder.init_array(l.len() as u32);
            for (i, e) in l.iter().enumerate() {
                toml_to_capnp(e, b.reborrow().get(i as u32))?;
            }
        }
        Value::Table(t) => {
            let mut kv = builder.init_table(t.len() as u32);
            for (i, (k, v)) in t.iter().enumerate() {
                let mut e = kv.reborrow().get(i as u32);
                e.set_key(k.as_str().into());
                toml_to_capnp(v, e.init_value())?;
            }
        }
    }

    Ok(())
}

fn value_to_struct(v: &Table, mut builder: ::capnp::dynamic_struct::Builder) -> Result<()> {
    'outer: for (k, v) in v.iter() {
        println!("{}", k);
        let mut builder = builder.reborrow();
        let field = builder.get_schema().get_field_by_name(k)?;

        // If we have reached a TOML value, dump the rest of the value
        if let capnp::schema_capnp::field::Slot(x) = field.get_proto().which()? {
            if let schema_capnp::type_::Which::Struct(s) = x.get_type()?.which()? {
                if s.get_type_id() == toml_capnp::value::Builder::TYPE_ID {
                    let dynamic: dynamic_struct::Builder = builder.init(field)?.downcast();
                    // FIXME
                    //toml_to_capnp(v, dynamic.downcast()?)?;
                    return Ok(());
                }
            }
        }

        match v {
            Value::String(s) => {
                if let TypeVariant::Enum(x) = field.get_type().which() {
                    let concrete: capnp::schema::EnumSchema = x.into();
                    for e in concrete.get_enumerants()? {
                        if s.eq_ignore_ascii_case(e.get_proto().get_name()?.to_str()?) {
                            builder.set(
                                field,
                                dynamic_value::Enum::new(e.get_ordinal(), concrete).into(),
                            )?;
                            continue 'outer;
                        }
                    }
                    return Err(eyre!("{:?} is not a valid enumeration value!", s));
                } else {
                    builder.set(field, s.as_str().into())?
                }
            }
            Value::Integer(i) => builder.set(field, (*i).into())?,
            Value::Float(f) => builder.set(field, (*f).into())?,
            Value::Boolean(b) => builder.set(field, (*b).into())?,
            Value::Datetime(d) => builder.set(field, d.to_string().as_str().into())?,
            Value::Array(l) => value_to_list(l, builder.initn(field, l.len() as u32)?.downcast())?,
            Value::Table(t) => value_to_struct(t, builder.init(field)?.downcast())?,
        }
    }

    Ok(())
}

pub fn to_capnp<'a, T: ::capnp::traits::OwnedStruct>(
    config: &Table,
    msg: T::Builder<'a>,
) -> Result<()>
where
    capnp::dynamic_value::Builder<'a>: From<T::Builder<'a>>,
{
    let dynamic: dynamic_value::Builder = msg.into();
    //let schema = T::introspect();
    //if let TypeVariant::Struct(x) = schema.which() {}

    Ok(value_to_struct(config, dynamic.downcast())?)
}

#[cfg(test)]
use crate::keystone_capnp::keystone_config;

#[test]
fn test_basic_config() -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();
    let source = r#"
database = "test.sqlite"
defaultLog = "debug"

[[modules]]
path = "/test/"
transient = false
config = { x = 1, y = 2 }

"#;

    to_capnp::<keystone_config::Owned>(&source.parse::<toml::Table>()?, msg.reborrow())?;
    println!("{:#?}", msg.reborrow_as_reader());

    Ok(())
}
