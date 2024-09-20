use capnp::private::capability::ClientHook;
use capnp::private::layout::ElementSize;
use capnp::private::layout::PointerBuilder;
use capnp::private::layout::PointerReader;
use capnp::private::layout::PointerType;
use capnp::private::layout::StructBuilder;
use capnp::private::layout::StructReader;
use std::future::Future;

pub struct CapReplacement<'a, F: FnMut(u64, capnp::any_pointer::Builder<'_>) -> capnp::Result<()>> {
    reader: PointerReader<'a>,
    evaluate: F,
}

impl<'a, F: FnMut(u64, capnp::any_pointer::Builder<'_>) -> capnp::Result<()>>
    capnp::traits::IntoInternalStructReader<'a> for CapReplacement<'a, F>
{
    fn into_internal_struct_reader(self) -> capnp::private::layout::StructReader<'a> {
        self.reader.get_struct(None).unwrap()
    }
}

fn copy_struct<F: FnMut(u64, capnp::any_pointer::Builder<'_>) -> capnp::Result<()>>(
    sr: &StructReader,
    sb: &mut StructBuilder,
    evaluate: &mut F,
) -> capnp::Result<()> {
    sb.copy_data_from(sr)?;
    for i in 0..sr.get_pointer_section_size() {
        process(
            sr.get_pointer_field(i as usize),
            sb.get_pointer_field_mut(i as usize),
            evaluate,
        )?;
    }
    Ok(())
}

pub fn process<F: FnMut(u64, capnp::any_pointer::Builder<'_>) -> capnp::Result<()>>(
    reader: PointerReader<'_>,
    mut builder: PointerBuilder<'_>,
    evaluate: &mut F,
) -> capnp::Result<()> {
    match reader.get_pointer_type()? {
        PointerType::Null => Ok(()),
        PointerType::Struct => {
            let sr = reader.get_struct(None)?;
            let mut sb = builder.init_struct(sr.struct_size());
            copy_struct(&sr, &mut sb, evaluate)
        }
        PointerType::List => {
            let rl = reader.get_list_any_size(None)?;

            match rl.get_element_size() {
                ElementSize::Pointer => {
                    let mut bl = builder.init_list(rl.get_element_size(), rl.len());
                    for i in 0..rl.len() {
                        process(
                            rl.get_pointer_element(i),
                            bl.reborrow().get_pointer_element(i),
                            evaluate,
                        )?;
                    }
                }
                ElementSize::InlineComposite => {
                    let mut bl = builder.init_struct_list(rl.len(), rl.get_element_struct_size());
                    for i in 0..rl.len() {
                        let sr = rl.get_struct_element(i);
                        let mut sb = bl.reborrow().get_struct_element(i);
                        copy_struct(&sr, &mut sb, evaluate)?;
                    }
                }
                _ => builder.copy_from(reader, false)?,
            }
            Ok(())
        }
        PointerType::Capability(index) => {
            evaluate(index as u64, capnp::any_pointer::Builder::new(builder))
        }
        PointerType::OtherPointer(value) => {
            evaluate(value, capnp::any_pointer::Builder::new(builder))
        }
    }
}

impl<'a, F: FnMut(u64, capnp::any_pointer::Builder<'_>) -> capnp::Result<()>>
    CapReplacement<'a, F>
{
    pub fn new(reader: capnp::any_pointer::Reader<'a>, evaluate: F) -> Self {
        let get: crate::proxy::GetPointerReader = reader.get_as().unwrap();
        Self {
            reader: get.reader,
            evaluate,
        }
    }
}

impl<F: FnMut(u64, capnp::any_pointer::Builder<'_>) -> capnp::Result<()>>
    capnp::traits::SetPointerBuilder for CapReplacement<'_, F>
{
    fn set_pointer_builder(
        builder: PointerBuilder<'_>,
        mut from: Self,
        _: bool,
    ) -> capnp::Result<()> {
        process(from.reader, builder, &mut from.evaluate)
    }
}

pub struct GetPointerBuilder<'a> {
    pub builder: capnp::private::layout::PointerBuilder<'a>,
}

impl<'a> capnp::traits::FromPointerBuilder<'a> for GetPointerBuilder<'a> {
    fn init_pointer(_: PointerBuilder<'a>, _: u32) -> Self {
        panic!("Cannot implement via GetPointerBuilder")
    }

    fn get_from_pointer(
        builder: PointerBuilder<'a>,
        _: Option<&'a [capnp::Word]>,
    ) -> capnp::Result<Self> {
        Ok(Self { builder })
    }
}
/*
fn replace_struct<
    F: Future<Output = capnp::Result<i64>> + 'static,
    FN: FnMut(Box<dyn ClientHook>) -> F,
>(
    callback: &mut FN,
    sb: &mut StructBuilder<'_>,
) -> capnp::Result<()> {
    let size = sb.reborrow().as_reader().get_pointer_section_size();
    for i in 0..size {
        replace(callback, sb.get_pointer_field_mut(i as usize))?;
    }
    Ok(())
}*/

pub async fn replace<
    F: Future<Output = capnp::Result<i64>> + 'static,
    FN: FnMut(Box<dyn ClientHook>) -> F,
>(
    callback: &mut FN,
    mut builder: PointerBuilder<'_>,
) -> capnp::Result<()> {
    let pointer = builder.reborrow().into_reader().get_pointer_type()?;
    match pointer {
        PointerType::Null => Ok(()),
        PointerType::Struct => {
            let size = builder
                .reborrow()
                .into_reader()
                .get_struct(None)?
                .struct_size();
            let mut sb = builder.get_struct(size, None)?;
            let size = sb.reborrow().as_reader().get_pointer_section_size();
            for i in 0..size {
                Box::pin(replace(callback, sb.get_pointer_field_mut(i as usize))).await?;
            }
            Ok(())
        }
        PointerType::List => {
            let element = builder
                .reborrow()
                .into_reader()
                .get_list_any_size(None)?
                .get_element_size();

            match element {
                ElementSize::Pointer => {
                    let size = builder
                        .reborrow()
                        .into_reader()
                        .get_list_any_size(None)?
                        .get_element_size();
                    let mut bl = builder.get_list(size, None)?;
                    for i in 0..bl.len() {
                        Box::pin(replace(callback, bl.reborrow().get_pointer_element(i))).await?;
                    }
                }
                ElementSize::InlineComposite => {
                    let size = builder
                        .reborrow()
                        .into_reader()
                        .get_list_any_size(None)?
                        .get_element_struct_size();
                    let mut bl = builder.get_struct_list(size, None)?;
                    for i in 0..bl.len() {
                        let mut sb = bl.reborrow().get_struct_element(i);
                        let size = sb.reborrow().as_reader().get_pointer_section_size();
                        for i in 0..size {
                            Box::pin(replace(callback, sb.get_pointer_field_mut(i as usize)))
                                .await?;
                        }
                    }
                }
                _ => (),
            }
            Ok(())
        }
        PointerType::OtherPointer(_) => Ok(()), // Nothing to replace here
        PointerType::Capability(_) => {
            let hook = builder.reborrow().into_reader().get_capability()?;
            let result = callback(hook).await?;
            unsafe { builder.set_other_pointer(result as u64) }
            Ok(())
        }
    }
}
