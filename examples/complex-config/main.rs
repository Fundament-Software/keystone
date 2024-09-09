include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

use crate::complex_config_capnp::config;
use crate::complex_config_capnp::root;
use capnp::any_pointer::Owned as any_pointer;
use capnp::private::capability::ClientHook;
use capnp::traits::Imbue;
use capnp::traits::ImbueMut;
use capnp::traits::Owned;

pub struct ComplexConfigImpl {
    pub msg: capnp::message::Builder<capnp::message::HeapAllocator>,
    pub caps: Vec<Option<Box<dyn ClientHook>>>,
}

impl root::Server for ComplexConfigImpl {
    async fn get_config(
        &self,
        _params: root::GetConfigParams,
        mut results: root::GetConfigResults,
    ) -> Result<(), ::capnp::Error> {
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

impl keystone::Module<complex_config_capnp::config::Owned> for ComplexConfigImpl {
    async fn new(
        config: <complex_config_capnp::config::Owned as Owned>::Reader<'_>,
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
        crate::complex_config_capnp::config::Owned,
        ComplexConfigImpl,
        crate::complex_config_capnp::root::Owned,
    >(async move {
        //let _: Vec<String> = ::std::env::args().collect();

        #[cfg(feature = "tracing")]
        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .init();
    })
    .await
}

#[cfg(test)]
use tempfile::NamedTempFile;

#[test]
fn test_complex_config() -> eyre::Result<()> {
    //console_subscriber::init();

    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .init();

    let temp_db = NamedTempFile::new().unwrap().into_temp_path();
    let temp_log = NamedTempFile::new().unwrap().into_temp_path();
    let temp_prefix = NamedTempFile::new().unwrap().into_temp_path();
    let mut source = keystone::build_temp_config(&temp_db, &temp_log, &temp_prefix);

    source.push_str(&keystone::build_module_config(
        "Complex Config",
        "complex-config-module",
        r#"{ nested = { state = [ "@keystone", "initCell", {id = "myCellName"}, "result" ], moreState = [ "@keystone", "initCell", {id = "myCellName"}, "result" ] } }"#,
    ));

    let (client_writer, server_reader) = async_byte_channel::channel();
    let (server_writer, client_reader) = async_byte_channel::channel();

    let pool = tokio::task::LocalSet::new();
    let a = pool.run_until(pool.spawn_local(keystone::start::<
        crate::complex_config_capnp::config::Owned,
        ComplexConfigImpl,
        crate::complex_config_capnp::root::Owned,
        async_byte_channel::Receiver,
        async_byte_channel::Sender,
    >(client_reader, client_writer)));

    let b = pool.run_until(pool.spawn_local(async move {
        let (mut instance, rpc, _disconnect, api) = keystone::Keystone::init_single_module(
            &source,
            "Complex Config",
            server_reader,
            server_writer,
        )
        .await
        .unwrap();

        let handle = tokio::task::spawn_local(rpc);
        let config_client: crate::complex_config_capnp::root::Client = api;

        tracing::debug!("Got API");
        let get_config = config_client.get_config_request();
        let get_response = get_config.send().promise.await?;
        tracing::debug!("Got Response");

        let response = get_response.get()?.get_reply()?;
        tracing::debug!("Got Reply");
        println!("{:#?}", response);

        tokio::select! {
            r = handle => r,
            _ = instance.shutdown() => Ok(Ok(())),
        }
        .unwrap()
        .unwrap();
        Ok::<(), capnp::Error>(())
    }));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async move {
        tokio::select! {
            r = a => r,
            r = b => r,
            r = tokio::signal::ctrl_c() => {
                    /*let handle = tokio::runtime::Handle::current();
                    if let Ok(dump) = tokio::time::timeout(tokio::time::Duration::from_secs(2), handle.dump()).await {
                        for (i, task) in dump.tasks().iter().enumerate() {
                            let trace = task.trace();
                            println!("TASK {i}:");
                            println!("{trace}\n");
                        }
                }*/
                eprintln!("Ctrl-C detected, aborting!");
                Ok(Ok(r.expect("failed to capture ctrl-c")))
            },
        }
    });

    runtime.shutdown_timeout(std::time::Duration::from_millis(1));
    result.unwrap().unwrap();

    Ok(())
}
