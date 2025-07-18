use capnp::private::capability::ClientHook;
use capnp::traits::Imbue;
use capnp::traits::ImbueMut;
use capnp::traits::Owned;
use capnp_macros::capnproto_rpc;
use complex_config::complex_config_capnp::config;
use complex_config::complex_config_capnp::root;
use keystone::capnp::any_pointer::Owned as any_pointer;
use keystone::{capnp, tokio};
use std::rc::Rc;

pub struct ComplexConfigImpl {
    pub msg: capnp::message::Builder<capnp::message::HeapAllocator>,
    pub caps: Vec<Option<Box<dyn ClientHook>>>,
}

#[capnproto_rpc(root)]
impl root::Server for ComplexConfigImpl {
    async fn get_config(self: Rc<Self>) -> Result<(), capnp::Error> {
        tracing::debug!("get_config was called! {:?}", self.caps);

        let mut reader: config::Reader = self.msg.get_root_as_reader()?;
        // You MUST re-imbue a message with it's cap table every time you get a new reader for it!
        // Yes this is completely insane.
        reader.imbue(&self.caps);
        let mut morecaps = Vec::new();
        results.get().imbue_mut(&mut morecaps);
        results.get().set_reply(reader)?;
        Ok(())
    }
}

impl keystone::Module<complex_config::complex_config_capnp::config::Owned> for ComplexConfigImpl {
    async fn new(
        config: <complex_config::complex_config_capnp::config::Owned as Owned>::Reader<'_>,
        _: keystone::keystone_capnp::host::Client<any_pointer>,
    ) -> capnp::Result<Self> {
        let mut msg = capnp::message::Builder::new_default();
        let mut caps = Vec::new();
        let mut builder: capnp::any_pointer::Builder = msg.init_root();
        builder.imbue_mut(&mut caps);
        builder.set_as(config)?;
        Ok(ComplexConfigImpl { msg, caps })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    keystone::main::<
        complex_config::complex_config_capnp::config::Owned,
        ComplexConfigImpl,
        complex_config::complex_config_capnp::root::Owned,
    >(async move {
        //let _: Vec<String> = ::std::env::args().collect();
    })
    .await
}

#[test]
fn test_complex_config() -> eyre::Result<()> {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .init();

    keystone::test_module_harness::<
        complex_config::complex_config_capnp::config::Owned,
        ComplexConfigImpl,
        complex_config::complex_config_capnp::root::Owned,
        _,
    >(
        &keystone::build_module_config(
            "Complex Config",
            "complex-config-module",
            r#"{ nested = { state = [ "@keystone", "initCell", {id = "myCellName"}, "result" ], moreState = [ "@keystone", "initCell", {id = "myCellName"}, "result" ] } }"#,
        ),
        "Complex Config",
        |api| async move {
            let config_client: complex_config::complex_config_capnp::root::Client = api;

            tracing::debug!("Got API");
            let get_config = config_client.get_config_request();
            let get_response = get_config.send().promise.await?;
            tracing::debug!("Got Response");

            let response = get_response.get()?.get_reply()?;
            tracing::debug!("Got Reply");
            println!("{:#?}", response);

            Ok::<(), eyre::Report>(())
        },
    )?;

    Ok(())
}
