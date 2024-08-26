#![allow(dead_code)]
mod binary_embed;
mod buffer_allocator;
mod byte_stream;
mod cap_replacement;
mod cap_std_capnproto;
mod cell;
mod config;
mod database;
pub mod host;
pub mod http;
mod keystone;
mod posix_module;
mod posix_process;
mod posix_spawn;
mod proxy;
mod sqlite;

include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

use crate::keystone_capnp::keystone_config;
use clap::{Args, Parser, Subcommand, ValueEnum};
use eyre::Result;
pub use keystone::*;
use std::{convert::Into, fs, io::Read, str::FromStr};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Default log level to use
    #[arg(short = 'l')]
    log: Option<LogLevel>,
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

impl From<LogLevel> for tracing_subscriber::filter::LevelFilter {
    fn from(val: LogLevel) -> Self {
        match val {
            LogLevel::Trace => tracing_subscriber::filter::LevelFilter::TRACE,
            LogLevel::Debug => tracing_subscriber::filter::LevelFilter::DEBUG,
            LogLevel::Info => tracing_subscriber::filter::LevelFilter::INFO,
            LogLevel::Warn => tracing_subscriber::filter::LevelFilter::WARN,
            LogLevel::Error => tracing_subscriber::filter::LevelFilter::ERROR,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Builds a configuration as a capnproto message. If no input is given, assumes stdin, and if no output is given, assumes stdout.
    Build {
        #[arg(short = 't')]
        toml: Option<String>,
        //#[arg(short = 'n')]
        //nickel: Option<String>,
        #[arg(short = 'o')]
        output: Option<String>,
    },
    /// Given any compiled capnproto message, converts it to a textual format. If no input is given, assumes stdin, and if no output is given, assumes stdout.
    Inspect {
        #[arg(short = 'm')]
        message: Option<String>,
        #[arg(short = 'o')]
        output: Option<String>,
    },
    /// Starts a new keystone session with the given compiled or textual config. If none are specified, loads "./keystone.config"
    Session {
        #[arg(short = 't')]
        toml: Option<String>,
        //#[arg(short = 'n')]
        //nickel: Option<String>,
        #[arg(short = 'c')]
        config: Option<String>,
    },
    /// Installs a new keystone daemon using the given compiled config.
    Install {
        /// If not specified, assumes the config lives in "./keystone.config"
        #[arg(short = 'c')]
        config: Option<String>,
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
    /// Generates a random file ID.
    Id {},
    /// Compiles a capnproto schema as a keystone module, automatically including the keystone standard schemas.
    Compile {
        /// List of files to compile
        files: Vec<String>,
        /// List of include directories to compile with
        #[arg(short = 'i')]
        include: Vec<String>,
        /// List of source prefixes to compile with
        #[arg(short = 'p')]
        prefix: Vec<String>,
        /// Disable standard include paths
        #[arg(short = 'n', long = "no-std")]
        no_std: bool,
    },
}

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen to shutdown signal");
}

fn inspect<R: Read>(
    reader: R,
) -> capnp::Result<capnp::message::Reader<capnp::serialize::OwnedSegments>> {
    let bufread = std::io::BufReader::new(reader);

    capnp::serialize::read_message(
        bufread,
        capnp::message::ReaderOptions {
            traversal_limit_in_words: None,
            nesting_limit: 128,
        },
    )
}

use std::io::Write;

#[allow(unused)]
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
        Commands::Build { toml, output } => {
            let mut message = ::capnp::message::Builder::new_default();
            let mut msg = message.init_root::<keystone_config::Builder>();
            let source = if let Some(t) = toml {
                fs::read_to_string(std::path::PathBuf::from_str(t.as_str())?)?
            } else {
                let mut source = Default::default();
                std::io::stdin().read_to_string(&mut source);
                source
            };

            config::to_capnp(&source.parse::<toml::Table>()?, msg.reborrow())?;

            if let Some(out) = output {
                let mut f = fs::File::create(out)?;
                capnp::serialize::write_message(f, &message);
            } else {
                capnp::serialize::write_message(std::io::stdout(), &message);
            }
        }
        Commands::Inspect { message, output } => {
            let msg = if let Some(path) = message {
                let file_contents = fs::read(path)?;

                let binary =
                    if let Ok(b) = crate::binary_embed::load_deps_from_binary(&file_contents) {
                        b
                    } else {
                        file_contents.as_slice()
                    };
                inspect(binary)?
            } else {
                inspect(std::io::stdin())?
            };

            let any: capnp::any_pointer::Reader = msg.get_root()?;
            let value: capnp::dynamic_value::Reader = any.into();
            if let Some(out) = output {
                let mut f = fs::File::create(out)?;
                write!(&mut f, "{:#?}", value);
            } else {
                print!("{:#?}", value);
            }
        }
        Commands::Session { toml, config } => {
            shutdown_signal().await;
            println!("Performing graceful shutdown...");
        }
        Commands::Id {} => {
            println!("0x{:x}", capnpc::generate_random_id());
        }
        _ => todo!(),
    }
    Ok(())
}
