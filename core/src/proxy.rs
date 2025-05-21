use crate::KeystoneRoot;
use crate::scheduler::Scheduler;
use crate::sqlite::SqliteDatabase;
use crate::util::SnowflakeSource;
use caplog::{CapLog, MAX_BUFFER_SIZE};
use capnp::MessageSize;
use capnp::capability::Client;
use capnp::capability::FromServer;
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

#[derive(Clone)]
pub struct ProxyServer {
    pub target: Client,
    pub set: Rc<RefCell<CapSet>>,
    pub log: Rc<RefCell<CapLog<MAX_BUFFER_SIZE>>>,
    snowflake: Rc<SnowflakeSource>,
    db: Rc<SqliteDatabase>,
    root: Rc<KeystoneRoot>,
    scheduler: Rc<Scheduler>,
}

impl ProxyServer {
    pub fn new(
        hook: Box<dyn ClientHook>,
        set: Rc<RefCell<CapSet>>,
        log: Rc<RefCell<CapLog<MAX_BUFFER_SIZE>>>,
        snowflake: Rc<SnowflakeSource>,
        db: Rc<SqliteDatabase>,
        root: Rc<KeystoneRoot>,
        scheduler: Rc<Scheduler>,
    ) -> Self {
        Self {
            target: Client::new(hook),
            set,
            log,
            snowflake,
            db,
            root,
            scheduler,
        }
    }
}

impl Server for ProxyServer {
    async fn dispatch_call(
        self,
        interface_id: u64,
        method_id: u16,
        params: Params<capnp::any_pointer::Owned>,
        mut results: Results<capnp::any_pointer::Owned>,
    ) -> Result<(), capnp::Error> {
        let p = params.hook.get()?;
        let reader: GetPointerReader = p.get_as()?;
        let caps = reader.reader.get_cap_table();

        let mut table = CapTable::with_capacity(caps.len());

        if self
            .log
            .borrow_mut()
            .append(
                self.snowflake.get(),
                self.snowflake.machine_id(),
                self.snowflake.instance_id(),
                0, // TODO: encapsulate this with the originating module and target module id
                p,
                p.target_size()
                    .unwrap_or(MessageSize {
                        word_count: 0,
                        cap_count: 0,
                    })
                    .word_count as usize,
            )
            .is_err()
        {
            eprintln!("Log failed in dispatch_call!");
        }

        for index in 0..caps.len() {
            table.push(if let Some(cap) = caps.extract_cap(index) {
                let client = Client::new(cap.add_ref());
                let client = capnp::capability::get_resolved_cap(client).await;
                Some(
                    if let Some(server) =
                        self.set.borrow_mut().get_local_server_of_resolved(&client)
                    {
                        if cap.get_brand() == self.target.hook.get_brand() {
                            // This is a proxy for a cap that belongs to this connection, so unwrap it
                            server.as_ref().target.hook.add_ref()
                        } else if cap.get_brand() == 0 {
                            // TODO
                            // Proxy for an internal keystone module from another RPC connection, so we
                            // simply create a new cap from our internal server for it.
                            cap
                        } else if cap.get_ptr() == Rc::as_ptr(&self.db) as usize {
                            Box::new(capnp_rpc::local::Client::new(
                                crate::sqlite_capnp::root::Client::from_rc(self.db.clone()),
                            ))
                        } else if cap.get_ptr() == Rc::as_ptr(&self.root) as usize {
                            Box::new(capnp_rpc::local::Client::new(
                                crate::keystone_capnp::root::Client::from_rc(self.root.clone()),
                            ))
                        } else if cap.get_ptr() == Rc::as_ptr(&self.scheduler) as usize {
                            Box::new(capnp_rpc::local::Client::new(
                                crate::scheduler_capnp::root::Client::from_rc(
                                    self.scheduler.clone(),
                                ),
                            ))
                        } else {
                            //  Proxy that should stay a proxy.
                            cap
                        }
                    } else if cap.get_brand() == self.target.hook.get_brand() {
                        // Not a proxy, belongs to either side of the RPC connection, so doesn't need a proxy
                        cap
                    } else {
                        // Not a proxy, belongs to some other RPC connection, needs a proxy
                        self.set
                            .borrow_mut()
                            .new_client(ProxyServer::new(
                                cap,
                                self.set.clone(),
                                self.log.clone(),
                                self.snowflake.clone(),
                                self.db.clone(),
                                self.root.clone(),
                                self.scheduler.clone(),
                            ))
                            .hook
                    },
                )
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

    fn get_ptr(&self) -> usize {
        self.target.hook.get_ptr()
    }
}
