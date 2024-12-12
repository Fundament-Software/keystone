use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::{fs::File, io::BufReader, path::Path};

use crate::keystone::Error;
use crate::keystone_capnp::cap_expr;
use crate::keystone_capnp::keystone_config;
use crate::toml_capnp;
use capnp::schema::CapabilitySchema;
use capnp::{
    dynamic_struct, dynamic_value, introspect::TypeVariant, schema::DynamicSchema, schema_capnp,
    traits::HasTypeId,
};
use eyre::Result;
use toml::{value::Offset, Table, Value};

fn expr_recurse(val: &Value, exprs: &mut HashMap<*const Value, u32>) {
    match val {
        Value::Array(a) => {
            if let Some(Value::String(k)) = a.first() {
                if k.starts_with('@') && !exprs.contains_key(&(val as *const Value)) {
                    exprs.insert(val as *const Value, exprs.len() as u32);
                }
            }
            // Not sure if we need to recurse here (this will always be a no-op on the first array element)
            //for v in a {
            //    expr_recurse(v, exprs)
            //}
        }
        Value::Table(t) => {
            for (_, v) in t {
                expr_recurse(v, exprs);
            }
        }
        _ => (),
    }
}

type SchemaVec = append_only_vec::AppendOnlyVec<(Vec<String>, DynamicSchema)>;

struct SchemaPool(SchemaVec);

impl SchemaPool {
    pub fn new() -> Self {
        Default::default()
    }

    fn get<'a>(&'a self, name: &str) -> Option<&'a DynamicSchema> {
        self.0
            .iter()
            .find(|(v, _)| v.iter().any(|file| name.eq_ignore_ascii_case(file)))
            .map(|(_, sc)| sc)
    }

    fn insert(&self, name: Option<&str>, schema: DynamicSchema) {
        if let Some(n) = name {
            self.0.push((vec![n.to_string()], schema));
        } else {
            let list = schema
                .get_files()
                .filter(|f| schema.get_type_by_scope(&["Root"], Some(f)).is_ok())
                .flat_map(|f| std::path::PathBuf::from_str(f));

            self.0.push((
                list.flat_map(|f| f.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .collect(),
                schema,
            ));
        }
    }
}

impl Default for SchemaPool {
    fn default() -> Self {
        Self(SchemaVec::new())
    }
}

fn value_to_list<F>(
    l: &[Value],
    mut builder: ::capnp::dynamic_list::Builder,
    schema: capnp::schema_capnp::type_::Reader,
    schemas: &SchemaPool,
    dir: &Path,
    callback: &mut F,
) -> Result<()>
where
    F: FnMut(*const Value) -> Option<u32>,
{
    // If this is a ModuleConfig list, call our handler function so we can look up the schema
    if let schema_capnp::type_::Which::List(s) = schema.which()? {
        if let schema_capnp::type_::Which::Struct(s) = s.get_element_type()?.which()? {
            if s.get_type_id()
                == keystone_config::module_config::Builder::<capnp::any_pointer::Owned>::TYPE_ID
            {
                for idx in 0..builder.len() {
                    let builder = builder.reborrow();
                    let dynamic: dynamic_struct::Builder = builder.get(idx)?.downcast();
                    if let Value::Table(t) = &l[idx as usize] {
                        toml_to_config(t, dynamic.downcast()?, schemas, dir, callback)?;
                    } else {
                        return Err(Error::InvalidTypeTOML(
                            "ModuleConfig List Element".into(),
                            "table".to_string(),
                        )
                        .into());
                    }
                }
                return Ok(());
            }
        }
    }

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
                    return Err(Error::InvalidEnumValue(s.into()).into());
                } else {
                    builder.set(idx, s.as_str().into())?
                }
            }
            Value::Integer(i) => builder.set(idx, (*i).into())?,
            Value::Float(f) => builder.set(idx, (*f).into())?,
            Value::Boolean(b) => builder.set(idx, (*b).into())?,
            Value::Datetime(d) => builder.set(idx, d.to_string().as_str().into())?,
            Value::Array(l) => {
                if let schema_capnp::type_::Which::List(s) = schema.which()? {
                    value_to_list(
                        l,
                        builder.init(idx, l.len() as u32)?.downcast(),
                        s.get_element_type()?,
                        schemas,
                        dir,
                        callback,
                    )?
                }
            }
            Value::Table(t) => {
                value_to_struct(t, builder.get(idx)?.downcast(), schemas, dir, callback)?
            }
        }
    }
    Ok(())
}

fn toml_to_capnp(v: &Value, mut builder: toml_capnp::value::Builder) -> Result<()> {
    match v {
        Value::String(s) => builder.set_toml_string(s.as_str().into()),
        Value::Integer(i) => builder.set_int(*i),
        Value::Float(f) => builder.set_float(*f),
        Value::Boolean(b) => builder.set_boolean(*b),
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

pub fn message_from_file(
    path: impl AsRef<Path>,
) -> capnp::Result<capnp::message::Reader<capnp::serialize::OwnedSegments>> {
    let f = File::open(path)?;
    let bufread = BufReader::new(f);
    capnp::serialize::read_message(
        bufread,
        capnp::message::ReaderOptions {
            traversal_limit_in_words: None,
            nesting_limit: 128,
        },
    )
}

fn toml_to_config<F>(
    v: &Table,
    mut builder: keystone_config::module_config::Builder<capnp::any_pointer::Owned>,
    schemas: &SchemaPool,
    dir: &Path,
    callback: &mut F,
) -> Result<()>
where
    F: FnMut(*const Value) -> Option<u32>,
{
    let path = v
        .get("path")
        .ok_or(Error::MissingFieldTOML("path".into()))?;

    if let Some(str) = path.as_str() {
        builder.set_path(str.into());
    }

    let path = Path::new(
        path.as_str()
            .ok_or(Error::InvalidTypeTOML("path".into(), "string".into()))?,
    );
    let path = dir.join(path);

    let mut name = None;

    if let Some(n) = v.get("name") {
        name = n.as_str();
        if let Some(str) = name {
            builder.set_name(str.into());
        }
    }

    if let Some(c) = v.get("config") {
        let msg = if let Some(schema) = v.get("schema") {
            if let Some(str) = schema.as_str() {
                builder.set_schema(str.into());
            }

            let schemafile = path.parent().unwrap_or(Path::new("")).join(
                schema
                    .as_str()
                    .ok_or(Error::InvalidTypeTOML("schema".into(), "string".into()))?,
            );

            message_from_file(schemafile)?
        } else {
            let file_contents = std::fs::read(path)?;

            let binary = crate::binary_embed::load_deps_from_binary(&file_contents)?;
            let bufread = BufReader::new(binary);
            capnp::serialize::read_message(
                bufread,
                capnp::message::ReaderOptions {
                    traversal_limit_in_words: None,
                    nesting_limit: 128,
                },
            )?
        };

        let anyconfig: capnp::any_pointer::Builder = builder.init_config();
        let schema = DynamicSchema::new(msg)?;
        let configtype = schema.get_type_by_scope(&["Config"], None)?;

        if let TypeVariant::Struct(st) = configtype {
            let dynobj: capnp::dynamic_struct::Builder = anyconfig.init_dynamic((*st).into())?;
            if let Value::Table(t) = c {
                if let Some(n) = name {
                    schemas.insert(Some(n), schema);
                }
                value_to_struct(t, dynobj, schemas, dir, callback)
            } else {
                Err(Error::InvalidTypeTOML("Config".into(), "table".into()).into())
            }
        } else {
            Err(Error::TypeMismatchCapstone("Config".into(), "struct".into()).into())
        }
    } else {
        Ok(())
    }
}

fn value_to_struct<F>(
    t: &Table,
    mut builder: dynamic_struct::Builder,
    schemas: &SchemaPool,
    dir: &Path,
    callback: &mut F,
) -> Result<()>
where
    F: FnMut(*const Value) -> Option<u32>,
{
    let mut bypass = HashSet::new();

    'outer: for (k, v) in t.iter() {
        let mut builder = builder.reborrow();
        let field = builder.get_schema().get_field_by_name(k)?;

        if let capnp::schema_capnp::field::Slot(x) = field.get_proto().which()? {
            // If we have reached a TOML value, dump the rest of the value
            if let schema_capnp::type_::Which::Struct(s) = x.get_type()?.which()? {
                if s.get_type_id() == toml_capnp::value::Builder::TYPE_ID {
                    let dynamic: dynamic_struct::Builder = builder.init(field)?.downcast();
                    toml_to_capnp(v, dynamic.downcast()?)?;
                    continue 'outer;
                }
            }

            // If we've reached a capability, halt TOML parsing and make sure all nested capabilities are registered
            if let schema_capnp::type_::Which::Interface(_) = x.get_type()?.which()? {
                // Register that we set this field so we don't autofill it
                bypass.insert(field.get_index());

                // TODO: refactor capnproto-rust so we don't have to do this
                unsafe {
                    if let Some(capid) = callback(v as *const Value) {
                        builder.set_capability_to_int(field, capid)?;
                    }
                }
                continue 'outer;
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
                    return Err(Error::InvalidEnumValue(s.clone()).into());
                } else {
                    builder.set(field, s.as_str().into())?
                }
            }
            Value::Integer(i) => builder.set(field, (*i).into())?,
            Value::Float(f) => builder.set(field, (*f).into())?,
            Value::Boolean(b) => builder.set(field, (*b).into())?,
            Value::Datetime(d) => builder.set(field, d.to_string().as_str().into())?,
            Value::Array(l) => {
                if let capnp::schema_capnp::field::Slot(x) = field.get_proto().which()? {
                    if let schema_capnp::type_::Which::AnyPointer(_) = x.get_type()?.which()? {
                        if l.first()
                            .map(|v| v.as_str().unwrap_or_default())
                            .map(|s| s.starts_with('@'))
                            .unwrap_or(false)
                        {
                            // Register that we set this field so we don't autofill it
                            bypass.insert(field.get_index());

                            // TODO: refactor capnproto-rust so we don't have to do this
                            unsafe {
                                if let Some(capid) = callback(v as *const Value) {
                                    builder.set_capability_to_int(field, capid)?;
                                }
                            }
                            continue 'outer;
                        }
                    }

                    value_to_list(
                        l,
                        builder.initn(field, l.len() as u32)?.downcast(),
                        x.get_type()?,
                        schemas,
                        dir,
                        callback,
                    )?
                } else {
                    return Err(Error::UnsupportedType(k.into(), "group".into()).into());
                }
            }
            Value::Table(t) => {
                value_to_struct(t, builder.init(field)?.downcast(), schemas, dir, callback)?
            }
        }
    }

    // Check if this struct has an autofill cell, but only if it wasn't already set
    for field in builder.get_schema().get_fields()? {
        if !bypass.contains(&field.get_index()) {
            for annotation in field.get_annotations()?.iter() {
                if annotation.get_id() == crate::module_capnp::autocell::ID {
                    // Add this as a null value pointer (or None if the unsafe pointer is ever turned into an optional reference)
                    unsafe {
                        builder
                            .set_capability_to_int(field, callback(std::ptr::null()).unwrap())?;
                    }
                    break;
                }
            }
        }
    }
    Ok(())
}

/*
struct CapExpr {
  union {
    moduleRef @0 :Text;
    field :group {
      base @1 :CapExpr;
      name @2 :Text;
    };
    method :group {
      subject @3 :CapExpr;
      name @4 :Text;
      args @5 :List(CapExpr);
    }
    literal @6 :Value;
    array @7 :List(CapExpr);
  }
}

Simple TOML example:
config = { my_mod_ref = [ "@indirect" ] }

Complex TOML example:
evaluates to my_mod_ref = @indirect.field1.method1(-2, "asdf", false, {another = "struct"}, @indirect2.field1).field2.method2().field3
config = { my_mod_ref = [ "@indirect", "field1", "method1", { a = -2, b = "asdf", c = false, d = { another = "struct" },  e = [ "@indirect2", "field1" ] }, "field2", "method2", {}, "field3" ] }
 */

fn toml_as_string(v: &Value) -> Result<&String, Error> {
    let Value::String(name) = v else {
        return Err(Error::InvalidConfig("Method name must be a string".into()));
    };
    Ok(name)
}

fn get_type_by_id<'a>(
    root: &'a DynamicSchema,
    id: u64,
    name: &str,
) -> Result<&'a TypeVariant, Error> {
    root.get_type_by_id(id)
        .ok_or(Error::MissingType(name.into(), id))
}

#[allow(clippy::too_many_arguments)]
fn build_cap_field<'a, F>(
    name: &str,
    expr: cap_expr::Builder<'a>,
    root: &DynamicSchema,
    schemas: &SchemaPool,
    callback: &mut F,
    list: &[Value],
    s: capnp::schema::StructSchema,
    dir: &Path,
) -> Result<cap_expr::Builder<'a>>
where
    F: FnMut(*const Value) -> Option<u32>,
{
    if list.is_empty() {
        return Ok(expr);
    }
    let fname = toml_as_string(&list[0])?;
    if let Some(f) = s.find_field_by_name(fname)? {
        // recurse if and only if we have items left
        let mut builder = if list.len() > 1 {
            if let capnp::schema_capnp::field::Slot(x) = f.get_proto().which()? {
                match x.get_type()?.which()? {
                    schema_capnp::type_::Which::Struct(st) => {
                        let dyntype = get_type_by_id(root, st.get_type_id(), fname)?;
                        if let TypeVariant::Struct(st) = dyntype {
                            build_cap_field(
                                fname,
                                expr,
                                root,
                                schemas,
                                callback,
                                &list[1..],
                                (*st).into(),
                                dir,
                            )?
                            .init_field()
                        } else {
                            return Err(Error::InvalidConfig(
                                "Tried to access subfield of something that isn't a struct".into(),
                            )
                            .into());
                        }
                    }
                    schema_capnp::type_::Which::Interface(cr) => {
                        if list.len() < 3 {
                            Err(Error::MissingMethodParameters(list[1].to_string()))?;
                        }
                        let variant = root
                            .get_type_by_id(cr.get_type_id())
                            .ok_or(Error::MissingSchemaField(list[1].to_string(), name.into()))?;

                        let TypeVariant::Capability(cs) = variant else {
                            return Err(Error::InvalidConfig(
                                "Schema type was not a capability".into(),
                            )
                            .into());
                        };

                        let cap = CapabilitySchema::new(*cs);

                        let Value::Table(t) = &list[2] else {
                            return Err(Error::InvalidConfig(
                                "Method parameters must be a table, and parameter names must be included (position parameters are not allowed)"
                                    .into(),
                            ).into());
                        };

                        // Evaluate the method call
                        eval_toml_capability(
                            name,
                            t,
                            expr,
                            cap,
                            root,
                            schemas,
                            callback,
                            &list[3..],
                            dir,
                        )?
                        .init_field()
                    }
                    _ => {
                        return Err(Error::InvalidConfig(
                            "Tried to access subfield of something that isn't a struct".into(),
                        )
                        .into());
                    }
                }
            } else {
                return Err(
                    Error::InvalidConfig("Tried to access subfield of group".into()).into(),
                );
            }
        } else {
            expr.init_field()
        };

        builder.set_index(f.get_index());
        Ok(builder.init_base())
    } else {
        Err(Error::MissingSchemaField(fname.to_string(), name.to_string()).into())
    }
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn build_cap_method<'a, F>(
    name: &str,
    v: &Table,
    interface_reader: capnp::schema_capnp::node::interface::Reader,
    id: u64,
    expr: cap_expr::Builder<'a>,
    root: &DynamicSchema,
    schemas: &SchemaPool,
    callback: &mut F,
    list: &[Value],
    dir: &Path,
) -> Result<Result<cap_expr::Builder<'a>, cap_expr::Builder<'a>>>
where
    F: FnMut(*const Value) -> Option<u32>,
{
    for (ordinal, method) in interface_reader.get_methods()?.into_iter().enumerate() {
        if method.get_name()? == name {
            let expr = if let TypeVariant::Struct(st) = root
                .get_type_by_id(method.get_result_struct_type())
                .ok_or(Error::MissingType(
                    format!("{} return values", name),
                    method.get_result_struct_type(),
                ))? {
                build_cap_field(name, expr, root, schemas, callback, list, (*st).into(), dir)?
            } else {
                return Err(Error::TypeMismatchCapstone(
                    format!("Results for {}", name),
                    "struct".into(),
                )
                .into());
            };

            let mut expr = expr.init_method();
            expr.set_method_id(ordinal as u16);
            expr.set_interface_id(id); // the node::reader::get_id() method is the type id

            let params =
                root.get_type_by_id(method.get_param_struct_type())
                    .ok_or(Error::MissingType(
                        format!("{} parameters", name),
                        method.get_result_struct_type(),
                    ))?;
            if let TypeVariant::Struct(st) = params {
                let builder = expr.reborrow().init_args();
                let dynobj = builder.init_dynamic((*st).into())?;
                value_to_struct(v, dynobj, schemas, dir, callback)?;
            } else {
                return Err(Error::TypeMismatchCapstone(
                    format!("Params for {}", name),
                    "struct".into(),
                )
                .into());
            }

            return Ok(Ok(expr.init_subject()));
        }
    }

    Ok(Err(expr))
}

#[allow(clippy::too_many_arguments)]
fn eval_toml_capability<'a, F>(
    k: &str,
    v: &Table,
    expr: cap_expr::Builder<'a>,
    schema: CapabilitySchema,
    root: &DynamicSchema,
    schemas: &SchemaPool,
    callback: &mut F,
    list: &[Value],
    dir: &Path,
) -> Result<cap_expr::Builder<'a>>
where
    F: FnMut(*const Value) -> Option<u32>,
{
    if let capnp::schema_capnp::node::Interface(interface_reader) = schema.get_proto().which()? {
        match build_cap_method(
            k,
            v,
            interface_reader,
            schema.get_proto().get_id(),
            expr,
            root,
            schemas,
            callback,
            list,
            dir,
        )? {
            Ok(builder) => return Ok(builder),
            Err(mut expr) => {
                let extends = interface_reader.get_superclasses()?;
                for superclass in extends {
                    if let TypeVariant::Capability(cs) = root
                        .get_type_by_id(superclass.get_id())
                        .ok_or(Error::InvalidConfig(format!(
                            "Couldn't find superclass {}!",
                            superclass.get_id()
                        )))?
                    {
                        // transform into a reader we can do something with
                        if let capnp::schema_capnp::node::Interface(interface_reader) =
                            CapabilitySchema::new(*cs).get_proto().which()?
                        {
                            expr = match build_cap_method(
                                k,
                                v,
                                interface_reader,
                                superclass.get_id(),
                                expr,
                                root,
                                schemas,
                                callback,
                                list,
                                dir,
                            )? {
                                Ok(builder) => return Ok(builder),
                                Err(expr) => expr,
                            }
                        }
                    }
                }
            }
        }
    }

    Err(Error::MissingMethod(
        k.into(),
        match schema.get_proto().get_display_name().map(|r| r.to_string()) {
            Ok(Ok(s)) => s,
            _ => format!("interface ID: {}", schema.get_proto().get_id()),
        },
    )
    .into())
}

fn compile_toml_expr<F>(
    v: &Value,
    mut expr: cap_expr::Builder,
    schemas: &SchemaPool,
    dir: &Path,
    callback: &mut F,
) -> Result<()>
where
    F: FnMut(*const Value) -> Option<u32>,
{
    // The root expr must always be a cap, because all expressions must begin with cap name of some kind
    if let Value::Array(l) = v {
        if l.is_empty() {
            Err(Error::InvalidConfig(
                "Capability reference cannot be an empty array".into(),
            ))?;
        }

        if let Value::String(k) = &l[0] {
            let module_name = k.strip_prefix('@').unwrap_or(k);

            match l.len() {
                1 => {
                    expr.reborrow().set_module_ref(module_name.into());
                    Ok(())
                }
                2 => Err(Error::MissingMethodParameters(l[1].to_string()))?,
                _ => {
                    let Some(schema) = schemas.get(module_name) else {
                        return Err(Error::MissingSchema(module_name.into()).into());
                    };

                    let variant = if let Ok(s) = schema.get_type_by_scope(&["Root"], None) {
                        s
                    } else {
                        // If no explicit name was provided, we should've generated possible names from the file_stems, so try to find it
                        let path = schema
                            .get_files()
                            .filter(|f| schema.get_type_by_scope(&["Root"], Some(f)).is_ok())
                            .find(|f| {
                                std::path::PathBuf::from_str(f)
                                    .map(|p| {
                                        p.file_stem()
                                            .map(|s| s.eq_ignore_ascii_case(module_name))
                                            .unwrap_or(false)
                                    })
                                    .unwrap_or(false)
                            });

                        if let Some(p) = path {
                            Ok(schema.get_type_by_scope(&["Root"], Some(p))?)
                        } else {
                            Err(Error::MissingSchema(module_name.into()))
                        }?
                    };

                    let Value::String(name) = &l[1] else {
                        return Err(
                            Error::InvalidConfig("Method name must be a string".into()).into()
                        );
                    };

                    let TypeVariant::Capability(cs) = variant else {
                        return Err(Error::InvalidConfig(
                            "Root schema was not a capability".into(),
                        )
                        .into());
                    };

                    let Value::Table(t) = &l[2] else {
                        return Err(Error::InvalidConfig(
                            "Method parameters must be a table, and parameter names must be included (position parameters are not allowed)"
                                .into(),
                        ).into());
                    };

                    let cap = CapabilitySchema::new(*cs);

                    // Evaluate the method call
                    let mut builder = eval_toml_capability(
                        name,
                        t,
                        expr.reborrow(),
                        cap,
                        schema,
                        schemas,
                        callback,
                        &l[3..],
                        dir,
                    )?;

                    builder.set_module_ref(module_name.into());
                    Ok(())
                }
            }
        } else {
            Err(Error::InvalidConfig(
                "First element of an array must be a Capability reference, which must be a string starting with @"
                    .into(),
            ).into())
        }
    } else {
        Err(Error::InvalidConfig("Capability reference must be a TOML array".into()).into())
    }
}

pub fn to_capnp(config: &Table, mut msg: keystone_config::Builder<'_>, dir: &Path) -> Result<()> {
    let span = tracing::span!(tracing::Level::DEBUG, "config::to_capnp", config = ?config);
    let _enter = span.enter();
    let schemas = builtin_schemas()?;
    let dynamic: dynamic_value::Builder = msg.reborrow().into();
    let mut exprs: HashMap<*const Value, u32> = HashMap::new();
    let exprs_ref = &mut exprs;
    value_to_struct(
        config,
        dynamic.downcast(),
        &schemas,
        dir,
        &mut |v| -> Option<u32> {
            if v.is_null() {
                exprs_ref.insert(v, exprs_ref.len() as u32);
                Some(exprs_ref[&v])
            } else {
                expr_recurse(unsafe { v.as_ref().unwrap() }, exprs_ref);
                if exprs_ref.contains_key(&v) {
                    Some(exprs_ref[&v])
                } else {
                    None
                }
            }
        },
    )?;

    // We've already identified all capexprs that need to be rooted in the cap table
    let mut builder = msg.init_cap_table(exprs.len() as u32);
    for (k, v) in &exprs {
        // A blank key means this cap index is the autogenerated state cell, so we leave it's entry blank
        if !k.is_null() {
            // Note: The lifetimes here do work out such that we could store a safe reference alongside the hashable pointer,
            // thus avoiding the unsafe as_ref() call, but this is simpler to implement for now.
            compile_toml_expr(
                unsafe { k.as_ref().unwrap() },
                builder.reborrow().get(*v),
                &schemas,
                dir,
                &mut |v| -> Option<u32> {
                    if exprs.contains_key(&v) {
                        Some(exprs[&v])
                    } else {
                        None
                    }
                },
            )?;
        }
    }

    Ok(())
}

const SELF_SCHEMA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/self.schema"));

/// Returns a pool containing all built-in schemas that keystone has been built with.
/// Currently this is simply everything in `schema/`, but could be modified at runtime.
fn builtin_schemas() -> capnp::Result<SchemaPool> {
    let schemas = SchemaPool::new();
    let bufread = BufReader::new(SELF_SCHEMA);

    let ks = DynamicSchema::new(capnp::serialize::read_message(
        bufread,
        capnp::message::ReaderOptions {
            traversal_limit_in_words: None,
            nesting_limit: 128,
        },
    )?)?;

    schemas.insert(None, ks);
    Ok(schemas)
}

#[test]
fn test_basic_config() -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();
    let source = r#"
database = "test.sqlite"
defaultLog = "debug"

[[modules]]
name = "test"
path = "/test/"

"#;

    to_capnp(
        &source.parse::<toml::Table>()?,
        msg.reborrow(),
        Path::new(""),
    )?;
    //println!("{:#?}", msg.reborrow_as_reader());

    Ok(())
}

#[test]
fn test_hello_world_config() -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();
    let source = format!(
        r#"
database = "test.sqlite"
defaultLog = "debug"

[[modules]]
name = "Hello World"
path = "{}"
config = {{ greeting = "Bonjour" }}
"#,
        crate::keystone::get_binary_path("hello-world-module")
            .as_os_str()
            .to_str()
            .unwrap()
            .replace('\\', "/")
    );

    to_capnp(
        &source.parse::<toml::Table>()?,
        msg.reborrow(),
        &std::env::current_dir()?,
    )?;
    //println!("{:#?}", msg.reborrow_as_reader());

    Ok(())
}

#[test]
fn test_indirect_config() -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();
    let source = format!(
        r#"
database = "test.sqlite"
defaultLog = "debug"

[[modules]]
name = "Hello World"
path = "{}"
config = {{ greeting = "Bonjour" }}

[[modules]]
name = "Indirect World"
path = "{}"
config = {{ helloWorld = [ "@Hello World" ] }}
"#,
        crate::keystone::get_binary_path("hello-world-module")
            .as_os_str()
            .to_str()
            .unwrap()
            .replace('\\', "/"),
        crate::keystone::get_binary_path("indirect-world-module")
            .as_os_str()
            .to_str()
            .unwrap()
            .replace('\\', "/")
    );

    to_capnp(
        &source.parse::<toml::Table>()?,
        msg.reborrow(),
        &std::env::current_dir()?,
    )?;
    //println!("{:#?}", msg.reborrow_as_reader());

    Ok(())
}

#[test]
fn test_stateful_config() -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();
    let source = format!(
        r#"
database = "test.sqlite"
defaultLog = "debug"

[[modules]]
name = "Stateful"
path = "{}"
config = {{ echoWord = "Echo" }}
"#,
        crate::keystone::get_binary_path("stateful-module")
            .as_os_str()
            .to_str()
            .unwrap()
            .replace('\\', "/")
    );

    to_capnp(
        &source.parse::<toml::Table>()?,
        msg.reborrow(),
        &std::env::current_dir()?,
    )?;
    //println!("{:#?}", msg.reborrow_as_reader());

    Ok(())
}

#[test]
fn test_complex_config() -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();
    let source = format!(
        r#"
database = "test.sqlite"
defaultLog = "debug"

[[modules]]
name = "Config Test"
path = "{}"
config = {{ nested = {{ state = [ "@keystone", "initCell", {{id = "myCellName"}}, "result" ], moreState = [ "@keystone", "initCell", {{id = "myCellName"}}, "result" ] }} }}
"#,
        crate::keystone::get_binary_path("complex-config-module")
            .as_os_str()
            .to_str()
            .unwrap()
            .replace('\\', "/")
    );

    to_capnp(
        &source.parse::<toml::Table>()?,
        msg.reborrow(),
        &std::env::current_dir()?,
    )?;
    let reader = msg.reborrow_as_reader();
    assert_eq!(reader.get_cap_table()?.len(), 2);
    //println!("{:#?}", msg.reborrow_as_reader());

    Ok(())
}

#[test]
fn test_config_array() -> Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    let mut msg = message.init_root::<keystone_config::Builder>();
    let source = format!(
        r#"
database = "test.sqlite"
defaultLog = "debug"

[[modules]]
name = "Sqlite Usage"
path = "{}"
config = {{ sqlite = [ "@sqlite" ], outer = [ "@keystone", "initCell", {{id = "OuterTableRef", default = ["@sqlite", "createTable", {{ def = [{{ name="state", baseType="text", nullable=false }}, {{ name="blah", baseType="text", nullable=false }}] }}, "res"]}}, "result" ], inner = [ "@keystone", "initCell", {{id = "InnerTableRef"}}, "result" ] }}
"#,
        crate::keystone::get_binary_path("sqlite-usage-module")
            .as_os_str()
            .to_str()
            .unwrap()
            .replace('\\', "/")
    );

    to_capnp(
        &source.parse::<toml::Table>()?,
        msg.reborrow(),
        &std::env::current_dir()?,
    )?;
    let reader = msg.reborrow_as_reader();
    assert_eq!(reader.get_cap_table()?.len(), 3);
    //println!("{:#?}", msg.reborrow_as_reader());

    Ok(())
}
