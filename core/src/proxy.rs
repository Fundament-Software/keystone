use capnp::capability::Client;
use capnp::capability::Params;
use capnp::capability::Request;
use capnp::capability::Results;
use capnp::capability::Server;
use capnp::private::capability::ClientHook;
use capnp::private::layout::CapTable;
use capnp::traits::FromPointerReader;
use capnp_rpc::CapabilityServerSet;
use std::cell::RefCell;
use std::rc::Rc;

pub type CapSet = CapabilityServerSet<ProxyServer, capnp::capability::Client>;

pub struct GetPointerReader<'a> {
    pub reader: capnp::private::layout::PointerReader<'a>,
}

impl<'a> FromPointerReader<'a> for GetPointerReader<'a> {
    fn get_from_pointer(
        reader: &capnp::private::layout::PointerReader<'a>,
        _: Option<&'a [capnp::Word]>,
    ) -> Result<Self, capnp::Error> {
        Ok(Self { reader: *reader })
    }
}

pub struct ProxyServer {
    pub target: Client,
    pub set: Rc<RefCell<CapSet>>,
}

impl ProxyServer {
    pub fn new(hook: Box<dyn ClientHook>, set: Rc<RefCell<CapSet>>) -> Self {
        Self {
            target: Client::new(hook),
            set,
        }
    }
}

impl Server for ProxyServer {
    async fn dispatch_call(
        &self,
        interface_id: u64,
        method_id: u16,
        params: Params<capnp::any_pointer::Owned>,
        mut results: Results<capnp::any_pointer::Owned>,
    ) -> Result<(), capnp::Error> {
        let p = params.hook.get()?;
        let reader: GetPointerReader = p.get_as()?;
        let caps = reader.reader.get_cap_table();
        let mut set = self.set.borrow_mut();

        let mut table = match caps {
            capnp::private::layout::CapTableReader::Plain(phooks) => {
                let hooks: &Vec<Option<Box<dyn ClientHook>>> = unsafe { &**phooks };
                CapTable::with_capacity(hooks.len())
            }
        };

        for index in 0..table.capacity() {
            table.push(if let Some(cap) = caps.extract_cap(index) {
                let client = Client::new(cap.add_ref());
                Some(if let Some(server) = set.get_local_server(&client).await {
                    if cap.get_brand() == self.target.hook.get_brand() {
                        // This is a proxy for a cap that belongs to this connection, so unwrap it
                        server.as_ref().target.hook.add_ref()
                    } else if cap.get_brand() == 0 {
                        // TODO
                        // Proxy for an internal keystone module from another RPC connection, so we
                        // simply create a new cap from our internal server for it.
                        cap
                    } else {
                        //  Proxy that should stay a proxy.
                        cap
                    }
                } else {
                    if cap.get_brand() == self.target.hook.get_brand() {
                        // Not a proxy, belongs to either side of the RPC connection, so doesn't need a proxy
                        cap
                    } else {
                        // Not a proxy, belongs to some other RPC connection, needs a proxy
                        // TODO: wait how does this know what connection it belongs to???
                        set.new_client(ProxyServer::new(cap, self.set.clone())).hook
                    }
                })
            } else {
                None
            });
        }

        let mut request: Request<capnp::any_pointer::Owned, capnp::any_pointer::Owned> = p
            .target_size()
            .map(|s| self.target.new_call(interface_id, method_id, Some(s)))?;
        request.hook.get().set_as(p)?;

        let response = request.send().promise.await?;
        results.hook.get()?.set_as(response.hook.get()?)?;
        Ok(())
    }
}
