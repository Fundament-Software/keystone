use crate::stateful_capnp::my_state;
use crate::stateful_capnp::root;
use keystone::storage_capnp::cell;

pub struct StatefulImpl {
    pub echo_word: String,
    pub echo_last: cell::Client<my_state::Owned>,
}

impl root::Server for StatefulImpl {
    async fn echo_last(
        &self,
        params: root::EchoLastParams,
        mut results: root::EchoLastResults,
    ) -> Result<(), ::capnp::Error> {
        tracing::debug!("echo_last was called!");
        let request = params.get()?.get_request()?;
        let name = request.get_name()?.to_str()?;
        let prev_request = self.echo_last.get_request().send();
        let prev_response = prev_request.promise.await?;
        let last_reader = prev_response.get()?.get_data()?.get_last()?;
        let last = last_reader.to_string()?;
        let word = self.echo_word.as_str();
        let message = format!("{word} {last}");

        let mut set_request = self.echo_last.set_request();
        let mut data = set_request.get().init_data();
        data.set_last(name.into());
        results.get().init_reply().set_message(message[..].into());
        set_request.send().promise.await?;
        Ok(())
    }
}

impl keystone::Module<crate::stateful_capnp::config::Owned> for StatefulImpl {
    async fn new(
        config: <crate::stateful_capnp::config::Owned as capnp::traits::Owned>::Reader<'_>,
        _: keystone::keystone_capnp::host::Client<capnp::any_pointer::Owned>,
    ) -> capnp::Result<Self> {
        Ok(StatefulImpl {
            echo_word: config.get_echo_word()?.to_string()?,
            echo_last: config.get_state()?,
        })
    }
}
