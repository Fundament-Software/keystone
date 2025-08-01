use capnp::capability::FromClientHook;
use capnp_macros::capnproto_rpc;
use keystone::capnp::any_pointer::Owned as any_pointer;
use keystone::sqlite_capnp::add_d_b;
use keystone::sqlite_capnp::d_b_any;
use keystone::sqlite_capnp::database;
use keystone::sqlite_capnp::expr::Expr;
use keystone::sqlite_capnp::expr::table_column::TableColumn;
use keystone::sqlite_capnp::insert;
use keystone::sqlite_capnp::insert::source;
use keystone::sqlite_capnp::join_clause;
use keystone::sqlite_capnp::r_a_table_ref;
use keystone::sqlite_capnp::r_o_database;
use keystone::sqlite_capnp::r_o_table_ref;
use keystone::sqlite_capnp::select;
use keystone::sqlite_capnp::select::ordering_term::OrderingTerm;
use keystone::sqlite_capnp::select_core;
use keystone::sqlite_capnp::table_field;
use keystone::sqlite_capnp::table_or_subquery;
use keystone::storage_capnp::cell;
use keystone::{capnp, tokio};
use sqlite_usage::sqlite_usage_capnp::root;
use std::rc::Rc;

pub struct SqliteUsageImpl {
    pub outer: cell::Client<keystone::sqlite_capnp::table_ref::Owned>,
    pub inner: keystone::sqlite_capnp::table::Client,
    pub sqlite: keystone::sqlite_capnp::root::Client,
}

#[capnproto_rpc(root)]
impl root::Server for SqliteUsageImpl {
    async fn echo_alphabetical(self: Rc<Self>, request: Reader) -> Result<(), capnp::Error> {
        tracing::debug!("echo_alphabetical was called!");

        let res = self
            .sqlite
            .clone()
            .cast_to::<r_o_database::Client>()
            .build_select_request(Some(select::Select {
                _selectcore: Some(Box::new(select_core::SelectCore {
                    _from: Some(join_clause::JoinClause {
                        _tableorsubquery: Some(table_or_subquery::TableOrSubquery::_Tableref(
                            self.inner.clone().cast_to::<r_o_table_ref::Client>(),
                        )),
                        _joinoperations: vec![],
                    }),
                    _results: vec![Expr::_Literal(d_b_any::DBAny::_Text("last"))],
                    _sql_where: Vec::new(),
                })),
                _mergeoperations: vec![],
                _orderby: vec![OrderingTerm {
                    _expr: Some(Expr::_Column(TableColumn {
                        _col_name: "last",
                        _reference: 0,
                    })),
                    _direction: select::ordering_term::AscDesc::Asc,
                }],
                _limit: None,
                _names: vec![],
            }))
            .send()
            .promise
            .await?;

        let next = res
            .get()?
            .get_res()?
            .build_next_request(1)
            .send()
            .promise
            .await?;

        let reader = next.get()?.get_res()?;
        let list = reader.get_results()?;
        let last = if !list.is_empty() {
            match list.get(0)?.get(0).which()? {
                d_b_any::Which::Text(s) => s?.to_str()?,
                _ => "<FAILURE>",
            }
        } else {
            "<No Previous Message>"
        };

        let message = format!("usage {last}");
        results.get().init_reply().set_message(message[..].into());

        let current = request.get_name()?.to_str()?;

        let ins = self
            .sqlite
            .clone()
            .cast_to::<database::Client>()
            .build_insert_request(Some(insert::Insert {
                _fallback: insert::ConflictStrategy::Fail,
                _target: self
                    .inner
                    .clone()
                    .cast_to::<r_a_table_ref::Client>()
                    .clone(),
                _source: source::Source::_Values(vec![vec![d_b_any::DBAny::_Text(current)]]),
                _cols: vec!["last"],
                _returning: Vec::new(),
            }))
            .send()
            .promise
            .await?;

        ins.get()?;
        Ok(())
    }
}

impl keystone::Module<sqlite_usage::sqlite_usage_capnp::config::Owned> for SqliteUsageImpl {
    async fn new(
        config: <sqlite_usage::sqlite_usage_capnp::config::Owned as capnp::traits::Owned>::Reader<
            '_,
        >,
        _: keystone::keystone_capnp::host::Client<any_pointer>,
    ) -> capnp::Result<Self> {
        let sqlite = config.get_sqlite()?;
        let inner = config.get_inner()?;

        let table = inner.get_request().send().promise.await?;
        let result = table.get()?;
        let table_cap = if !result.has_data() {
            let create_table_request = sqlite
                .clone()
                .client
                .cast_to::<add_d_b::Client>()
                .build_create_table_request(vec![table_field::TableField {
                    _name: "last",
                    _base_type: table_field::Type::Text,
                    _nullable: false,
                }]);

            create_table_request
                .send()
                .promise
                .await?
                .get()?
                .get_res()?
        } else {
            result.get_data()?
        };

        let mut set_request = inner.set_request();
        set_request.get().set_data(table_cap.clone())?;
        set_request.send().promise.await?;

        Ok(SqliteUsageImpl {
            inner: table_cap,
            outer: config.get_outer()?,
            sqlite,
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    keystone::main::<sqlite_usage::sqlite_usage_capnp::config::Owned, SqliteUsageImpl, root::Owned>(
        async move {
            //let _: Vec<String> = ::std::env::args().collect();
        },
    )
    .await
}

#[test]
fn test_sqlite_usage() -> eyre::Result<()> {
    //console_subscriber::init();

    keystone::test_module_harness::<
        sqlite_usage::sqlite_usage_capnp::config::Owned,
        SqliteUsageImpl,
        sqlite_usage::sqlite_usage_capnp::root::Owned,
        _,
    >(
        &keystone::build_module_config(
            "Sqlite Usage",
            "sqlite-usage-module",
            r#"{ sqlite = [ "@sqlite" ], outer = [ "@keystone", "initCell", {id = "OuterTableRef", default = ["@sqlite", "createTable", { def = [{ name="state", baseType="text", nullable=false }] }, "res"]}, "result" ], inner = [ "@keystone", "initCell", {id = "InnerTableRef"}, "result" ] }"#,
        ),
        "Sqlite Usage",
        |api| async move {
            let usage_client: sqlite_usage::sqlite_usage_capnp::root::Client = api;

            {
                let mut echo = usage_client.echo_alphabetical_request();
                echo.get().init_request().set_name("3 Keystone".into());
                let echo_response = echo.send().promise.await?;

                let msg = echo_response.get()?.get_reply()?.get_message()?;

                assert_eq!(msg, "usage <No Previous Message>");
            }

            {
                let mut echo = usage_client.echo_alphabetical_request();
                echo.get().init_request().set_name("2 Replace".into());
                let echo_response = echo.send().promise.await?;

                let msg = echo_response.get()?.get_reply()?.get_message()?;

                assert_eq!(msg, "usage 3 Keystone");
            }

            {
                let mut echo = usage_client.echo_alphabetical_request();
                echo.get().init_request().set_name("1 Reload".into());
                let echo_response = echo.send().promise.await?;

                let msg = echo_response.get()?.get_reply()?.get_message()?;

                assert_eq!(msg, "usage 2 Replace");
            }
            Ok::<(), eyre::Error>(())
        },
    )?;

    Ok(())
}
