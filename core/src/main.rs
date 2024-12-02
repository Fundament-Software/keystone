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
pub mod scheduler;
pub mod sqlite;
mod sturdyref;
mod util;

use crate::keystone_capnp::keystone_config;
use circular_buffer::CircularBuffer;
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::event::KeyCode::Char;
use crossterm::event::{Event, KeyEvent};
use eyre::Result;
use futures_util::{FutureExt, StreamExt};
pub use keystone::*;
use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::{convert::Into, fs, io::Read};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

#[cfg(not(target_os = "windows"))]
use crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags};

include!(concat!(env!("OUT_DIR"), "/capnproto.rs"));

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
        #[arg(short = 'i')]
        interactive: bool,
    },
    /// Installs a new keystone daemon using the given compiled config.
    Install {
        /// If not specified, assumes the config lives in "./keystone.config"
        #[arg(short = 'c')]
        config: Option<String>,
        /// If specified, copies the entire directory next to the keystone executable. Will eventually be replaced with a proper content store.
        #[arg(short = 't')]
        store: Option<String>,
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

#[inline]
pub async fn drive_stream_with_error(
    msg: &str,
    stream: &mut futures_util::stream::FuturesUnordered<impl Future<Output = eyre::Result<()>>>,
) {
    while let Some(r) = stream.next().await {
        if let Err(e) = r {
            eprintln!("{}: {}", msg, e);
        }
    }
}

fn keystone_startup(
    dir: &Path,
    message: keystone_config::Reader<'_>,
    interactive: bool,
) -> Result<()> {
    let pool = tokio::task::LocalSet::new();
    let mut instance = Keystone::new(message, false)?;

    let fut = pool.run_until(async move {
        instance.init(dir, message).await?;

        // TODO: Eventually the terminal interface should be factored out into a module using a monitoring capability.
        if interactive {
            if let Err(e) = run_interface(&mut instance).await {
                eprintln!("{}", e.to_string());
            }
        } else {
            tokio::select! {
                _ = drive_stream_with_error("Module crashed!", &mut instance.rpc_systems) => (),
                r = tokio::signal::ctrl_c() => r.expect("failed to listen to shutdown signal"),
            };
        }

        eprintln!("Attempting graceful shutdown...");
        let (mut shutdown, rpc_systems) = instance.shutdown();

        tokio::join!(
            drive_stream_with_error("Error during shutdown!", &mut shutdown),
            drive_stream_with_error("Error during shutdown!", rpc_systems)
        );

        Ok::<(), eyre::Report>(())
    });

    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(fut);
    runtime.shutdown_timeout(std::time::Duration::from_millis(1000));
    result?;
    Ok(())
}

enum TerminalEvent {
    Evt(Event),
    Render,
    Tick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabPage {
    Keystone = 0,
    Module = 1,
    Network = 2,
    //Dependencies,
}

static TABPAGES: [TabPage; 3] = [TabPage::Keystone, TabPage::Module, TabPage::Network];

impl std::string::ToString for TabPage {
    fn to_string(&self) -> String {
        match self {
            TabPage::Keystone => "Keystone",
            TabPage::Module => "Module",
            TabPage::Network => "Network",
        }
        .to_owned()
    }
}

struct TuiKeystone {
    name: String,
    cpu: CircularBuffer<32, u64>,
    ram: CircularBuffer<32, u64>,
    links: u32,
    modules: u32,
    loaded: u32,
    state: ModuleState,
    selected: i32,
}

struct TuiModule {
    name: String,
    state: ModuleState,
}

struct TuiNetworkNode {
    keystone: TuiKeystone,
    health: f32,
}

struct Tui<'a> {
    tab: TabPage,
    keystone: TuiKeystone,
    module: Option<TuiModule>,
    network: Vec<TuiNetworkNode>,
    modules: &'a mut std::collections::HashMap<u64, ModuleInstance>,
}

impl ratatui::widgets::Widget for &Tui<'_> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        use ratatui::prelude::*;
        use ratatui::symbols::border;
        use ratatui::widgets::*;

        let style_green = Style::new().green();
        let style_yellow = Style::new().yellow();
        let style_red = Style::new().red();

        let title = Line::from(" Keystone Status ".bold());
        let instructions = Line::from(vec![
            " Quit ".into(),
            "<Q>".green().bold(),
            " — Scroll ".into(),
            "<⭡⭣>".green().bold(),
            " — Select ".into(),
            "<Enter>".green().bold(),
            " — Back ".into(),
            "<Esc>".green().bold(),
            " — Next Page ".into(),
            "<Tab> ".green().bold(),
        ]);

        let cell = Block::bordered()
            .border_set(border::PLAIN)
            .borders(Borders::RIGHT);

        let tabarea = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(area);

        let tab_select = Style::new().black().on_white().bold();
        let tab_unselect = Style::new().white();
        let tab_disable = Style::new().dark_gray();

        let tabs = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(TABPAGES.map(|_| Constraint::Min(1)))
            .split(tabarea[0]);

        for tab in TABPAGES.iter() {
            let style = if self.tab == *tab {
                tab_select
            } else if *tab == TabPage::Module && self.module.is_none() {
                tab_disable
            } else {
                tab_unselect
            };

            Paragraph::new(Span::styled(tab.to_string(), style)).render(tabs[*tab as usize], buf);
        }

        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered())
            .border_set(border::THICK);
        block.render(tabarea[1], buf);

        let main = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(tabarea[1]);

        let block = Block::bordered()
            .border_set(border::PLAIN)
            .borders(Borders::BOTTOM);
        block.render(main[1], buf);

        let header = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(6),
                Constraint::Min(5),
                Constraint::Min(5),
                Constraint::Min(12),
                Constraint::Min(8),
            ])
            .split(main[0]);

        Paragraph::new(format!("Name: {}", self.keystone.name))
            .block(cell.clone())
            .render(header[0], buf);

        let halves = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(5), Constraint::Min(1)])
            .split(header[1]);

        Paragraph::new("CPU: ").render(halves[0], buf);
        Sparkline::default()
            .data(self.keystone.cpu.iter())
            .render(halves[1], buf);
        cell.clone().render(header[1], buf);

        let halves = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(5), Constraint::Min(1)])
            .split(header[2]);

        Paragraph::new("RAM: ").render(halves[0], buf);
        Sparkline::default()
            .data(self.keystone.ram.iter())
            .render(halves[1], buf);
        cell.clone().render(header[2], buf);

        Paragraph::new(Line::from(vec![
            Span::raw("Modules: "),
            Span::styled("⬤", style_green),
            Span::raw(format!(
                " {}/{}",
                self.keystone.loaded, self.keystone.modules
            )),
        ]))
        .block(cell.clone())
        .render(header[3], buf);

        Paragraph::new(Line::from(vec![
            Span::raw("State: "),
            Span::styled("⬤", style_green),
            Span::raw(format!(" {}", self.keystone.state.to_string())),
        ]))
        .render(header[4], buf);
    }
}

impl Tui<'_> {
    fn key(&mut self, key: KeyEvent, cancellation_token: &CancellationToken) {
        use crossterm::event::KeyCode;
        use crossterm::event::KeyEventKind;

        match key.code {
            Char('q') => {
                if key.kind == KeyEventKind::Press {
                    cancellation_token.cancel();
                }
            }
            KeyCode::Up if key.kind != KeyEventKind::Release => {
                if self.modules.len() > 0 {
                    self.keystone.selected =
                        (self.keystone.selected + 1) % self.modules.len() as i32;
                }
            }
            KeyCode::Down if key.kind != KeyEventKind::Release => {
                if self.modules.len() > 0 {
                    if self.keystone.selected < 0 {
                        self.keystone.selected = 0;
                    }
                    self.keystone.selected =
                        (self.keystone.selected - 1) % self.modules.len() as i32;
                }
            }
            KeyCode::Tab if key.kind != KeyEventKind::Release => {
                self.tab = TABPAGES[((self.tab as usize) + 1) % TABPAGES.len()];
                if self.tab == TabPage::Module && self.module.is_none() {
                    self.tab = TABPAGES[((self.tab as usize) + 1) % TABPAGES.len()];
                }
            }
            _ => (),
        }
    }
}

async fn event_loop<B: ratatui::prelude::Backend>(
    modules: &mut std::collections::HashMap<u64, ModuleInstance>,
    mut terminal: ratatui::Terminal<B>,
    mut event_rx: UnboundedReceiver<TerminalEvent>,
    cancellation_token: CancellationToken,
) -> Result<()> {
    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

    let mut app = Tui {
        tab: TabPage::Keystone,
        keystone: TuiKeystone {
            name: gethostname::gethostname().to_string_lossy().to_string(),
            cpu: CircularBuffer::new(),
            ram: CircularBuffer::new(),
            links: 0,
            modules: modules.len() as u32,
            loaded: 0,
            state: ModuleState::NotStarted,
            selected: -1,
        },
        module: None,
        network: Vec::new(),
        modules,
    };

    let mut sys =
        System::new_with_specifics(RefreshKind::new().with_cpu(CpuRefreshKind::everything()));

    while let Some(evt) = event_rx.recv().await {
        match evt {
            TerminalEvent::Evt(e) => match e {
                Event::Key(key) => app.key(key, &cancellation_token),
                Event::Mouse(mouse) => (),
                Event::FocusGained => (),
                Event::FocusLost => (),
                Event::Paste(_) => (),
                Event::Resize(x, y) => (),
            },
            TerminalEvent::Render => terminal
                .draw(|frame| frame.render_widget(&app, frame.area()))
                .map(|_| ())?,
            TerminalEvent::Tick => {
                sys.refresh_memory_specifics(MemoryRefreshKind::new().with_ram());
                sys.refresh_cpu_all();
                let mut total = 0.0;
                for cpu in sys.cpus() {
                    total += cpu.cpu_usage();
                }
                app.keystone.cpu.push_front((total * 100.0) as u64);
                app.keystone.ram.push_front(sys.used_memory());
                app.keystone.loaded = 0;
                app.keystone.state = ModuleState::NotStarted;

                for (i, m) in &mut *app.modules {
                    match m.state {
                        ModuleState::Ready | ModuleState::Paused => app.keystone.loaded += 1,
                        ModuleState::Initialized
                            if app.keystone.state == ModuleState::NotStarted =>
                        {
                            app.keystone.state = ModuleState::Initialized
                        }
                        _ => (),
                    }
                }

                if app.keystone.loaded == app.keystone.modules {
                    app.keystone.state = ModuleState::Ready;
                }
            }
        }
    }

    Ok(())
}

async fn event_stream(
    stream: &mut futures_util::stream::FuturesUnordered<impl Future<Output = eyre::Result<()>>>,
    event_tx: UnboundedSender<TerminalEvent>,
    cancellation_token: CancellationToken,
) -> Result<()> {
    const TICK_RATE: f64 = 4.0;
    const FRAME_RATE: f64 = 60.0;

    let mut tick_interval =
        tokio::time::interval(std::time::Duration::from_secs_f64(1.0 / TICK_RATE));
    let mut render_interval =
        tokio::time::interval(std::time::Duration::from_secs_f64(1.0 / FRAME_RATE));
    let mut events = crossterm::event::EventStream::new();
    loop {
        let tick_delay = tick_interval.tick();
        let render_delay = render_interval.tick();
        let crossterm_event = events.next().fuse();
        tokio::select! {
            _ = cancellation_token.cancelled() => break,
            _ = tokio::signal::ctrl_c() => cancellation_token.cancel(),
            evt = crossterm_event => {
                if let Some(e) = evt {
                    event_tx.send(TerminalEvent::Evt(e?))?;
                }
            },
            _ = tick_delay => event_tx.send(TerminalEvent::Tick)?,
            _ = render_delay => event_tx.send(TerminalEvent::Render)?,
            r = stream.next() => if let Some(Err(e)) = r {
                eprintln!("Module crashed! {}", e);
            } else if r.is_none() {
                cancellation_token.cancel();
            }
        }
    }

    Ok(())
}

async fn run_interface(instance: &mut Keystone) -> Result<()> {
    let mut terminal = ratatui::init();
    terminal.clear()?;

    let cancellation_token = CancellationToken::new();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<TerminalEvent>();

    // Catch the error so we always restore the terminal state no matter what happens
    let e = tokio::try_join!(
        event_stream(
            &mut instance.rpc_systems,
            event_tx,
            cancellation_token.clone()
        ),
        event_loop(
            &mut instance.modules,
            terminal,
            event_rx,
            cancellation_token.clone()
        ),
    );
    ratatui::restore();
    e?;
    Ok(())
}

#[allow(unused)]
fn main() -> Result<()> {
    // Setup eyre
    color_eyre::install()?;

    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_max_level(cli.log.unwrap_or(LogLevel::Warn))
        //.with_max_level(cli.log.unwrap_or(LogLevel::Trace))
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
            let mut parent = None;
            let source = if let Some(t) = toml.as_ref() {
                let path = std::path::Path::new(t.as_str());
                parent = path.parent();
                fs::read_to_string(path)?
            } else {
                let mut source = Default::default();
                std::io::stdin().read_to_string(&mut source);
                source
            };

            config::to_capnp(
                &source.parse::<toml::Table>()?,
                msg.reborrow(),
                parent.unwrap_or(Path::new("")),
            )?;

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
        Commands::Session {
            toml,
            config,
            interactive,
        } => {
            if let Some(p) = toml {
                let path = Path::new(&p);
                let mut f = std::fs::File::open(path)?;
                let mut buf = String::new();
                f.read_to_string(&mut buf)?;

                let mut message = ::capnp::message::Builder::new_default();
                let mut msg = message.init_root::<keystone_config::Builder>();
                let dir = path.parent().unwrap_or(Path::new(""));
                config::to_capnp(&buf.parse::<toml::Table>()?, msg.reborrow(), dir)?;
                keystone_startup(
                    dir,
                    message.get_root_as_reader::<keystone_config::Reader>()?,
                    interactive,
                )
            } else if let Some(p) = config {
                let path = Path::new(&p);
                keystone_startup(
                    path.parent().unwrap_or(Path::new("")),
                    crate::config::message_from_file(path)?
                        .get_root::<keystone_config::Reader>()?,
                    interactive,
                )
            } else {
                keystone_startup(
                    &std::env::current_dir()?,
                    crate::config::message_from_file(Path::new("./keystone.config"))?
                        .get_root::<keystone_config::Reader>()?,
                    interactive,
                )
            }?;
        }
        Commands::Id {} => {
            println!("0x{:x}", capnpc::generate_random_id());
        }
        _ => todo!(),
    }
    Ok(())
}
