use capnp::private::layout::ElementSize;
use capnp::private::layout::PointerBuilder;
use capnp::private::layout::PointerReader;
use capnp::private::layout::PointerType;
use capnp::private::layout::StructBuilder;
use capnp::private::layout::StructReader;

pub struct CapReplacement<'a, F: Fn(u32, capnp::any_pointer::Builder<'_>) -> capnp::Result<()>> {
    reader: PointerReader<'a>,
    evaluate: F,
}

impl<'a, F: Fn(u32, capnp::any_pointer::Builder<'_>) -> capnp::Result<()>> CapReplacement<'a, F> {
    pub fn new(reader: capnp::any_pointer::Reader<'a>, evaluate: F) -> Self {
        let get: crate::proxy::GetPointerReader = reader.get_as().unwrap();
        Self {
            reader: get.reader,
            evaluate,
        }
    }

    fn copy_struct(sr: &StructReader, sb: &mut StructBuilder, evaluate: &F) -> capnp::Result<()> {
        sb.copy_data_from(sr)?;
        for i in 0..sr.get_pointer_section_size() {
            Self::process(
                sr.get_pointer_field(i as usize),
                sb.get_pointer_field_mut(i as usize),
                evaluate,
            )?;
        }
        Ok(())
    }

    pub fn process(
        reader: PointerReader<'_>,
        mut builder: PointerBuilder<'_>,
        evaluate: &F,
    ) -> capnp::Result<()> {
        match reader.get_pointer_type()? {
            PointerType::Null => Ok(()),
            PointerType::Struct => {
                let sr = reader.get_struct(None)?;
                let mut sb = builder.init_struct(sr.struct_size());
                Self::copy_struct(&sr, &mut sb, evaluate)
            }
            PointerType::List => {
                let rl = reader.get_list_any_size(None)?;

                match rl.get_element_size() {
                    ElementSize::Pointer => {
                        let mut bl = builder.init_list(rl.get_element_size(), rl.len());
                        for i in 0..rl.len() {
                            Self::process(
                                rl.get_pointer_element(i),
                                bl.reborrow().get_pointer_element(i),
                                evaluate,
                            )?;
                        }
                    }
                    ElementSize::InlineComposite => {
                        let mut bl =
                            builder.init_struct_list(rl.len(), rl.get_element_struct_size());
                        for i in 0..rl.len() {
                            let sr = rl.get_struct_element(i);
                            let mut sb = bl.reborrow().get_struct_element(i);
                            Self::copy_struct(&sr, &mut sb, evaluate)?;
                        }
                    }
                    _ => builder.copy_from(reader, false)?,
                }
                Ok(())
            }
            PointerType::Capability(index) => {
                evaluate(index, capnp::any_pointer::Builder::new(builder))
            }
        }
    }
}

impl<F: Fn(u32, capnp::any_pointer::Builder<'_>) -> capnp::Result<()>>
    capnp::traits::SetPointerBuilder for CapReplacement<'_, F>
{
    fn set_pointer_builder(builder: PointerBuilder<'_>, from: Self, _: bool) -> capnp::Result<()> {
        Self::process(from.reader, builder, &from.evaluate)
    }
}
