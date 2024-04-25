#![allow(dead_code)]
mod byte_stream;
mod cap_std_capnproto;
mod config;
mod database;
mod keystone;
mod posix_module;
mod posix_spawn;
mod spawn;

capnp_import::capnp_import!("schema/**/*.capnp");

#[cfg(test)]
capnp_import::capnp_import!("../modules/hello-world/*.capnp");

use crate::keystone_capnp::keystone_config;
use capnp::{dynamic_value, introspect::Introspect};
use clap::{Args, Parser, Subcommand, ValueEnum};
use eyre::Result;
use std::convert::Into;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Default log level to use
    #[arg(short = 'l')]
    log: Option<LogLevel>,

    /// Auth password for running a command that connects to an existing keystone session.
    #[arg(short = 'p')]
    password: Option<String>,

    /// SSH key for running a command that connects to an existing keystone session.
    #[arg(short = 'k')]
    key: Option<String>,

    /// If connecting to an existing keystone session that is not using the default socket, name of the socket to use.
    #[arg(short = 'n')]
    name: Option<String>,
}

#[derive(ValueEnum, Copy, Clone, Debug, PartialEq, Eq)]
enum LogLevel {
    /// Designates very low priority, often extremely verbose, information.
    Trace,
    /// Designates lower priority information.
    Debug,
    /// Designates useful information.
    Info,
    /// Designates hazardous situations.
    Warn,
    /// Designates very serious errors.
    Error,
}

impl Into<tracing_subscriber::filter::LevelFilter> for LogLevel {
    fn into(self) -> tracing_subscriber::filter::LevelFilter {
        match self {
            Self::Trace => tracing_subscriber::filter::LevelFilter::TRACE,
            Self::Debug => tracing_subscriber::filter::LevelFilter::DEBUG,
            Self::Info => tracing_subscriber::filter::LevelFilter::INFO,
            Self::Warn => tracing_subscriber::filter::LevelFilter::WARN,
            Self::Error => tracing_subscriber::filter::LevelFilter::ERROR,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Compiles a TOML configuration to a sqlite database. If no database is provided, uses the one in the config file itself.
    Compile {
        #[arg(short = 'c')]
        config: String,
        #[arg(short = 'o')]
        out: Option<String>,
    },
    /// Given a keystone database, dumps the config to a TOML file. If no database is provided, tries to find a running keystone server.
    Dump {
        #[arg(short = 'd')]
        database: Option<String>,
        #[arg(short = 'o')]
        out: String,
    },
    /// Starts a new keystone session with the given database or config.
    Session {
        #[arg(short = 'd')]
        database: Option<String>,
        #[arg(short = 'c')]
        config: String,
    },
    /// If an existing keystone daemon has been installed and is not currently running, starts it.
    Start {},
    /// If an existing keystone daemon has been installed and is currently running, stops it.
    Stop {
        /// WARNING: MAY CAUSE DATA LOSS. Force stops the instance, not allowing modules to cleanly shut down.
        #[arg(short = 'f')]
        force: bool,
    },
    /// Installs a new keystone daemon using the given precompiled database.
    Install {
        #[arg(short = 'd')]
        database: Option<String>,
        /// If any modules are specified in both the old and new configs, preserve their state and internal configuration.
        #[arg(short = 'u')]
        update: bool,
        /// If a keystone daemon is already installed, overwrite it completely.
        #[arg(short = 'o')]
        overwrite: bool,
        /// If a keystone daemon is already running, try to gracefully close it first.
        #[arg(short = 's')]
        stop: bool,
        /// WARNING: MAY CAUSE DATA LOSS. If a keystone daemon is already running, forcibly kill it before updating.
        #[arg(short = 'f')]
        force: bool,
    },
    /// If a keystone daemon is installed, uninstall it.
    Uninstall {
        /// If a keystone daemon is already running, try to gracefully close it first.
        #[arg(short = 's')]
        stop: bool,
        /// WARNING: MAY CAUSE DATA LOSS. If a keystone daemon is already running, forcibly kill it before updating.
        #[arg(short = 'f')]
        force: bool,
    },
    /// Run a CapnProto subcommand, like capnp id.
    #[command(arg_required_else_help = true)]
    Capnp(CapNPArgs),
    /// Inspect or interact with any loaded keystone modules using their public CapnProto API
    Module(ModuleCommandArgs),
}

/// Temporarily hold our module command
#[derive(Args, Clone, Debug)]
struct ModuleCommandArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    _args: Vec<String>,
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
struct CapNPArgs {
    #[command(subcommand)]
    command: Option<CapNPCommands>,
}

#[derive(Debug, Subcommand)]
enum CapNPCommands {
    /// Generates a random schema ID
    ID {},
    /// Compiles a capnproto schema using the given language driver
    Compile {},
    /// Converts capnproto schemas between formats
    Convert {},
    /// Evaluates a capnproto constant inside a schema file.
    Eval { schema_file: String, name: String },
}

//async fn hello_world(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
//    Ok(Response::new("Hello, World".into()))
//}

#[async_backtrace::framed]
async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen to shutdown signal");
}

#[async_backtrace::framed]
#[tokio::main]
async fn main() -> Result<()> {
    // Setup eyre
    color_eyre::install()?;

    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_max_level(cli.log.unwrap_or(LogLevel::Warn))
        .with_target(true)
        .with_timer(tracing_subscriber::fmt::time::OffsetTime::new(
            time::UtcOffset::UTC,
            time::format_description::well_known::Rfc3339,
        ))
        .init();

    match cli.command {
        Commands::Compile { config, out } => {
            let mut message = ::capnp::message::Builder::new_default();
            let mut msg = message.init_root::<keystone_config::Builder>();
            let source = std::fs::read_to_string(config)?;

            config::to_capnp::<keystone_config::Owned>(
                &source.parse::<toml::Table>()?,
                msg.reborrow(),
            )?;
            println!("{:#?}", msg.reborrow_as_reader());
        }
        Commands::Dump { database, out } => {
            println!("TODO!!!");
        }
        Commands::Session { database, config } => {
            shutdown_signal().await;
            println!("Performing graceful shutdown...");
        }
        Commands::Module(ModuleCommandArgs { _args }) => {
            //
            println!("TODO!!!: {:?}", _args);
        }
        _ => todo!(),
    }
    // load config files
    // Keystone is provided with a sqlite database on boot, which contains all the configuration necessary to bootstrap it
    // while this bootstrap configuration can be anything, per-module configurations must be capnproto structs because they
    // must be able to save sturdyref capabilities. Our bootstrap configuration also contains capability references, but only
    // by calling the result of another interface.
    Ok(())
}
