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
mod module;
mod posix_module;
mod posix_process;
mod posix_spawn;
mod proxy;
pub mod scheduler;
pub mod sqlite;
mod sturdyref;
mod util;

use crate::byte_stream::ByteStreamBufferImpl;
use crate::keystone_capnp::keystone_config;
use capnp::any_pointer::Owned as any_pointer;
use capnp::introspect::Introspect;
use capnp::{dynamic_struct, dynamic_value};
use circular_buffer::CircularBuffer;
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::event::KeyCode::Char;
use crossterm::event::{Event, KeyEvent};
use eyre::Result;
use futures_util::{FutureExt, StreamExt};
pub use keystone::*;
pub use module::*;
use ratatui::widgets::{ListState, TableState};
use std::collections::BTreeMap;
use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::pin::Pin;
use std::{convert::Into, fs, io::Read};
use tokio::io::AsyncReadExt;
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
    let (mut instance, mut rpc_systems) = Keystone::new(message, false)?;

    let fut = pool.run_until(async move {
        // TODO: Eventually the terminal interface should be factored out into a module using a monitoring capability.
        if interactive {
            let (log_tx, log_rx) = mpsc::unbounded_channel::<(u64, String)>();
            let log_tx_copy = log_tx.clone();
            instance
                .init_custom(dir, message.reborrow(), &rpc_systems, move |id| {
                    let tx = log_tx_copy.clone();
                    log_capture(id, tx)
                })
                .await?;
            if let Err(e) = run_interface(
                &mut instance,
                &mut rpc_systems,
                dir,
                message,
                log_rx,
                log_tx,
            )
            .await
            {
                eprintln!("{}", e.to_string());
            }
        } else {
            instance
                .init(
                    dir,
                    message.reborrow(),
                    &rpc_systems,
                    keystone::Keystone::passthrough_stderr,
                )
                .await?;
            tokio::select! {
                _ = drive_stream_with_error("Module crashed!", &mut rpc_systems) => (),
                r = tokio::signal::ctrl_c() => r.expect("failed to listen to shutdown signal"),
            };
        }

        eprintln!("Attempting graceful shutdown...");
        let mut shutdown = instance.shutdown();

        tokio::join!(
            drive_stream_with_error("Error during shutdown!", &mut shutdown),
            drive_stream_with_error("Error during shutdown RPC!", &mut rpc_systems)
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
    Interface = 3,
    Input = 4,
}

static TABPAGES: [TabPage; 5] = [
    TabPage::Keystone,
    TabPage::Module,
    TabPage::Network,
    TabPage::Interface,
    TabPage::Input,
];

impl std::string::ToString for TabPage {
    fn to_string(&self) -> String {
        match self {
            TabPage::Keystone => "Keystone",
            TabPage::Module => "Module",
            TabPage::Interface => "API Explorer",
            TabPage::Network => "Network",
            TabPage::Input => "Input",
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
    table_state: TableState,
}

struct TuiModule {
    id: u64,
    cpu: CircularBuffer<32, u64>,
    ram: CircularBuffer<32, u64>,
    name: String,
    state: ModuleState,
    selected: Option<usize>,
    trace: tracing::Level,
    log: CircularBuffer<512, String>,
}

impl TuiModule {
    fn rotate_trace(&self) -> tracing::Level {
        match self.trace {
            tracing::Level::TRACE => tracing::Level::DEBUG,
            tracing::Level::DEBUG => tracing::Level::INFO,
            tracing::Level::INFO => tracing::Level::WARN,
            tracing::Level::WARN => tracing::Level::ERROR,
            tracing::Level::ERROR => tracing::Level::TRACE,
        }
    }
}

struct TuiNetworkNode {
    keystone: TuiKeystone,
    health: f32,
}

struct Tui<'a> {
    tab: TabPage,
    keystone: TuiKeystone, // TODO: Replace this with a reference into the network when we have a way to identify keystone nodes
    module: Option<u64>,
    network: Vec<TuiNetworkNode>,
    modules: BTreeMap<u64, TuiModule>,
    instance: &'a mut Keystone,
    dir: &'a Path,
    config: keystone_config::Reader<'a>,
    list_state: ListState,
    log_tx: UnboundedSender<(u64, String)>,
    holding: Vec<ParamResultType>,
    input: RowCol,
    buffer: String, //TODO optimize input stuff
}

struct RowCol {
    row: usize,
    col: usize,
}

fn invert_style(selected: bool, style: ratatui::prelude::Style) -> ratatui::prelude::Style {
    if selected {
        style.add_modifier(ratatui::prelude::Modifier::REVERSED)
    } else {
        style
    }
}

impl ratatui::widgets::Widget for &mut Tui<'_> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        use ratatui::prelude::*;
        use ratatui::symbols::border;
        use ratatui::widgets::*;

        let style_green = Style::new().green();
        let style_yellow = Style::new().yellow();
        let style_red = Style::new().red();
        let style_gray = Style::new().dark_gray();

        let state_style = |state: ModuleState| match state {
            ModuleState::NotStarted => style_gray,
            ModuleState::Initialized => style_yellow,
            ModuleState::Ready => style_green,
            ModuleState::Paused => style_yellow,
            ModuleState::Closing => style_yellow,
            ModuleState::Closed => style_gray,
            ModuleState::Aborted => style_red,
            ModuleState::StartFailure => style_red,
            ModuleState::CloseFailure => style_red,
        };

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
            } else if (*tab == TabPage::Module) && self.module.is_none() {
                tab_disable
            } else {
                tab_unselect
            };

            Paragraph::new(Span::styled(tab.to_string(), style)).render(tabs[*tab as usize], buf);
        }

        match self.tab {
            TabPage::Keystone => {
                let title = Line::from(" Keystone Status ".bold());
                let instructions = Line::from(vec![
                    " Quit ".into(),
                    "<Q>".green().bold(),
                    " — Scroll ".into(),
                    "<⭡⭣>".green().bold(),
                    " — Select ".into(),
                    "<Enter>".green().bold(),
                    " — Next Page ".into(),
                    "<Tab> ".green().bold(),
                ]);

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
                    Span::styled(
                        "⬤",
                        if self.keystone.loaded == 0 {
                            style_red
                        } else if self.keystone.loaded == self.keystone.modules {
                            style_green
                        } else {
                            style_yellow
                        },
                    ),
                    Span::raw(format!(
                        " {}/{}",
                        self.keystone.loaded, self.keystone.modules
                    )),
                ]))
                .block(cell.clone())
                .render(header[3], buf);

                Paragraph::new(Line::from(vec![
                    Span::raw("State: "),
                    Span::styled("⬤", state_style(self.keystone.state)),
                    Span::raw(format!(" {}", self.keystone.state.to_string())),
                ]))
                .render(header[4], buf);

                const HEADER: [&str; 6] = ["ID", "Name", "State", "CPU", "RAM", "Last Log"];

                let rows = self.modules.iter().map(|(id, m)| {
                    Row::new([
                        Cell::from(Text::from(id.to_string())),
                        Cell::from(Text::from(m.name.as_str())),
                        Cell::from(Line::from(vec![
                            Span::styled("⬤ ", state_style(m.state)),
                            Span::raw(m.state.to_string()),
                        ])),
                        Cell::from(Text::from("<TODO>")),
                        Cell::from(Text::from("<TODO>")),
                        Cell::from(
                            Text::from(m.log.front().map(|s| s.as_str()).unwrap_or(""))
                                .right_aligned(),
                        ),
                    ])
                    .height(1)
                });

                let table = StatefulWidget::render(
                    Table::new(rows, HEADER.map(|_| Constraint::Min(1)))
                        .column_spacing(0)
                        // It has an optional header, which is simply a Row always visible at the top.
                        .header(Row::new(HEADER).style(Style::new().bold()))
                        .row_highlight_style(Style::new().reversed()),
                    main[2],
                    buf,
                    &mut self.keystone.table_state,
                );
            }
            TabPage::Module if self.module.is_some() => {
                let Some(id) = self.module.as_ref() else {
                    unreachable!();
                };
                let Some(module) = self.modules.get(id) else {
                    return;
                };

                let title = Line::from(" Module Status ".bold());
                let instructions = Line::from(vec![
                    " Quit ".into(),
                    "<Q>".green().bold(),
                    " — Pick ".into(),
                    "<←→>".green().bold(),
                    " — Select ".into(),
                    "<Enter>".green().bold(),
                    " — Back ".into(),
                    "<Esc>".green().bold(),
                    " — Next Page ".into(),
                    "<Tab> ".green().bold(),
                ]);

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
                        Constraint::Length(1),
                        Constraint::Min(1),
                    ])
                    .split(tabarea[1]);

                let header = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Min(6),
                        Constraint::Min(5),
                        Constraint::Min(5),
                        Constraint::Min(8),
                    ])
                    .split(main[0]);

                Paragraph::new(format!("Name: {}", module.name))
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
                    Span::raw("State: "),
                    Span::styled("⬤", state_style(module.state)),
                    Span::raw(format!(" {}", module.state.to_string())),
                ]))
                .render(header[3], buf);

                let actionbar = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(10),
                        Constraint::Min(15),
                        Constraint::Length(18),
                        Constraint::Length(13),
                        Constraint::Min(5),
                    ])
                    .split(main[1]);

                Paragraph::new("Commands: ").render(actionbar[0], buf);

                let tabs = match module.state {
                    ModuleState::NotStarted
                    | ModuleState::Closed
                    | ModuleState::Aborted
                    | ModuleState::StartFailure
                    | ModuleState::CloseFailure => vec![Line::from(Span::styled(
                        "Start ⏵",
                        Style::new().green().bold(),
                    ))],
                    ModuleState::Initialized | ModuleState::Ready => vec![
                        Line::from(Span::styled("Stop ▪", Style::new().red().bold())),
                        Line::from(Span::styled("Pause ॥", Style::new().yellow().bold())),
                        Line::from(Span::styled("Restart ↺", Style::new().blue().bold())),
                    ],
                    ModuleState::Paused => vec![
                        Line::from(Span::styled("Stop ▪", Style::new().red().bold())),
                        Line::from(Span::styled("Start ⏵", Style::new().green().bold())),
                        Line::from(Span::styled("Restart ↺", Style::new().blue().bold())),
                    ],
                    ModuleState::Closing => vec![Line::from(Span::styled(
                        "Kill 🕱",
                        Style::new().red().bold(),
                    ))],
                };
                let count = tabs.len();

                Tabs::new(tabs)
                    .highlight_style(Style::new().black().on_white().bold())
                    .divider(symbols::DOT)
                    .select(module.selected)
                    .render(actionbar[1], buf);

                let mut is_selected = if let Some(x) = module.selected.map(|s| s == (count + 0)) {
                    x
                } else {
                    false
                };

                Paragraph::new(Span::styled(
                    "[API Explorer]",
                    match module.state {
                        ModuleState::NotStarted
                        | ModuleState::Closed
                        | ModuleState::Aborted
                        | ModuleState::StartFailure
                        | ModuleState::CloseFailure
                        | ModuleState::Closing => Style::new().dark_gray().bold(),
                        ModuleState::Initialized | ModuleState::Ready | ModuleState::Paused => {
                            let sel = is_selected;

                            is_selected = if let Some(x) = module.selected.map(|s| s == (count + 1))
                            {
                                x
                            } else {
                                false
                            };

                            invert_style(sel, Style::new().white().bold())
                        }
                    },
                ))
                .render(actionbar[2], buf);

                Paragraph::new("Trace Level: ").render(actionbar[3], buf);

                Paragraph::new(match module.trace {
                    tracing::Level::TRACE => Span::styled(
                        "[· TRACE]",
                        invert_style(is_selected, Style::new().gray().bold()),
                    ),
                    tracing::Level::DEBUG => Span::styled(
                        "[⁇ DEBUG]",
                        invert_style(is_selected, Style::new().white().bold()),
                    ),
                    tracing::Level::INFO => Span::styled(
                        "[🛈 INFO]",
                        invert_style(is_selected, Style::new().blue().bold()),
                    ),
                    tracing::Level::WARN => Span::styled(
                        "[❢ WARNING]",
                        invert_style(is_selected, Style::new().yellow().bold()),
                    ),
                    tracing::Level::ERROR => Span::styled(
                        "[🗙 ERROR]",
                        invert_style(is_selected, Style::new().red().bold()),
                    ),
                })
                .render(actionbar[4], buf);

                let block = Block::bordered()
                    .border_set(border::PLAIN)
                    .borders(Borders::BOTTOM);
                block.render(main[2], buf);

                // Ratatui doesn't like it when a stateful widget stops existing, so we render a blank one.
                let table = StatefulWidget::render(
                    Table::default(),
                    main[3],
                    buf,
                    &mut self.keystone.table_state,
                );

                let items: Vec<_> = module
                    .log
                    .iter()
                    .map(|s| Line::from(vec![Span::raw(s.as_str())]))
                    .collect();
                let list = List::new(items).direction(ratatui::widgets::ListDirection::TopToBottom);
                ratatui::prelude::Widget::render(list, main[3], buf);
            }
            TabPage::Network => {
                let title = Line::from(" Network Status ".bold());
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

                let block = Block::bordered()
                    .title(title.centered())
                    .title_bottom(instructions.centered())
                    .border_set(border::THICK);
                block.render(tabarea[1], buf);

                let main = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints([Constraint::Length(1), Constraint::Min(1)])
                    .split(tabarea[1]);

                let list = List::new(vec![Line::from(vec![
                    Span::styled("☗ ", state_style(self.keystone.state)),
                    Span::raw(self.keystone.name.as_str()),
                ])])
                .direction(ratatui::widgets::ListDirection::TopToBottom);

                StatefulWidget::render(list, main[1], buf, &mut self.list_state);
            }
            TabPage::Interface => {
                let main = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints([Constraint::Length(10)])
                    .split(tabarea[1]);
                let mut rows: Vec<Row> = Vec::new();
                let mut widths = Vec::new();
                //TODO either change widths to look better or swap off of table
                for desc in self.instance.cap_functions.iter_mut() {
                    let mut inner = Vec::new();
                    if desc.type_id == 0 {
                        inner.push(Cell::from(desc.function_name.clone()));
                        widths.push(Constraint::Max(desc.function_name.len() as u16));
                        if let ModuleOrCap::ModuleId(id) = &desc.module_or_cap {
                            if let Some(m) = self.modules.get(id) {
                                match m.state {
                                    ModuleState::NotStarted => {
                                        let s = ": not started".to_string();
                                        widths.push(Constraint::Max(s.len() as u16));
                                        inner.push(Cell::from(s));
                                    }
                                    ModuleState::Initialized => {
                                        let s = ": initialized".to_string();
                                        widths.push(Constraint::Max(s.len() as u16));
                                        inner.push(Cell::from(s));
                                    }
                                    ModuleState::Ready => {
                                        let s = ": ready".to_string();
                                        widths.push(Constraint::Max(s.len() as u16));
                                        inner.push(Cell::from(s));
                                    }
                                    ModuleState::Paused => {
                                        let s = ": paused".to_string();
                                        widths.push(Constraint::Max(s.len() as u16));
                                        inner.push(Cell::from(s));
                                    }
                                    ModuleState::Closing => {
                                        let s = ": closing".to_string();
                                        widths.push(Constraint::Max(s.len() as u16));
                                        inner.push(Cell::from(s));
                                    }
                                    ModuleState::Closed => {
                                        let s = ": closed".to_string();
                                        widths.push(Constraint::Max(s.len() as u16));
                                        inner.push(Cell::from(s));
                                    }
                                    ModuleState::Aborted => {
                                        let s = ": aborted".to_string();
                                        widths.push(Constraint::Max(s.len() as u16));
                                        inner.push(Cell::from(s));
                                    }
                                    ModuleState::StartFailure => {
                                        let s = ": start failure".to_string();
                                        widths.push(Constraint::Max(s.len() as u16));
                                        inner.push(Cell::from(s));
                                    }
                                    ModuleState::CloseFailure => {
                                        let s = ": close failure".to_string();
                                        widths.push(Constraint::Max(s.len() as u16));
                                        inner.push(Cell::from(s));
                                    }
                                }
                            }
                        }
                        rows.push(Row::new(inner));
                        continue;
                    }
                    widths.push(Constraint::Max(2 + desc.function_name.len() as u16));
                    inner.push(Cell::from(format!("{} (", desc.function_name.clone())));
                    for param in desc.params.iter_mut() {
                        if let CapnpType::Struct(st) = &mut param.capnp_type {
                            if st.fields.is_empty() {
                                for field in st.schema.get_fields().unwrap() {
                                    st.fields.push(ParamResultType {
                                        name: field
                                            .get_proto()
                                            .get_name()
                                            .unwrap()
                                            .to_string()
                                            .unwrap(),
                                        capnp_type: field.get_type().which().into(),
                                    });
                                }
                            }
                        }
                        let p = param.clone().to_string();
                        widths.push(Constraint::Max(p.len().try_into().unwrap()));
                        inner.push(Cell::from(p));
                    }
                    inner.push(Cell::from(") -> ("));
                    widths.push(Constraint::Max(6));
                    let mut results = String::new();
                    for result in desc.results.iter_mut() {
                        if let CapnpType::Struct(st) = &mut result.capnp_type {
                            if st.fields.is_empty() {
                                for field in st.schema.get_fields().unwrap() {
                                    st.fields.push(ParamResultType {
                                        name: field
                                            .get_proto()
                                            .get_name()
                                            .unwrap()
                                            .to_string()
                                            .unwrap(),
                                        capnp_type: field.get_type().which().into(),
                                    });
                                }
                            }
                        }
                        let r = result.clone().to_string();
                        widths.push(Constraint::Max(r.len().try_into().unwrap()));
                        inner.push(Cell::from(r));
                    }
                    inner.push(Cell::from(")"));
                    widths.push(Constraint::Max(1));
                    rows.push(Row::new(inner));
                }

                let title = Line::from(" API ".bold());
                let holding = if let Some(t) = self.holding.last() {
                    format!("[{}]", t.clone().to_string().as_str())
                } else {
                    "[]".to_string()
                };
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
                    "<->".green().bold(),
                    " — Clone ".into(),
                    "<=>".green().bold(),
                    " — Set ".into(),
                    holding.as_str().green().bold(),
                    " — Currently Holding ".into(),
                ]);

                let block = Block::bordered()
                    .title(title.centered())
                    .title_bottom(instructions.centered())
                    .border_set(border::THICK);
                block.render(tabarea[1], buf);

                let mut widths = Vec::new();
                widths.push(Constraint::Percentage(8));
                for _ in 0..8 {
                    widths.push(Constraint::Percentage(20)); //TODO table won't work, has to be some multi list solution instead
                }
                widths.push(Constraint::Percentage(1));
                let table = StatefulWidget::render(
                    Table::new(rows, widths)
                        .column_spacing(1)
                        .cell_highlight_style(Style::new().reversed()),
                    main[0],
                    buf,
                    &mut self.keystone.table_state,
                );

                //StatefulWidget::render(table, main[0], buf, &mut self.keystone.table_state);
            }
            TabPage::Input => {
                let mut desc = &mut self.instance.cap_functions[self.input.row];
                let title = Line::from(" Input ".bold());
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

                let block = Block::bordered()
                    .title(title.centered())
                    .title_bottom(instructions.centered())
                    .border_set(border::THICK);
                block.render(tabarea[1], buf);

                let main = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints([Constraint::Length(1), Constraint::Min(1)])
                    .split(tabarea[1]);
                let mut fields = Vec::new();
                if self.input.col != 0 {
                    let mut desc = &mut self.instance.cap_functions[self.input.row];
                    let param = &desc.params[self.input.col - 1];
                    match &param.capnp_type {
                        CapnpType::Data(items) => {
                            field_data_helper(&mut fields, items, param.name.as_str())
                        }
                        CapnpType::Struct(st) => {
                            field_struct_helper(&mut fields, st, param.name.as_str())
                        }
                        CapnpType::List(capnp_types) => {
                            field_list_helper(&mut fields, capnp_types, param.name.as_str())
                        }
                        _ => todo!(),
                    }
                }

                let list = List::new(fields)
                    .direction(ratatui::widgets::ListDirection::TopToBottom)
                    .highlight_style(Style::new().reversed());

                StatefulWidget::render(list, main[1], buf, &mut self.list_state);
            }
            _ => (),
        }
    }
}
fn field_data_helper(fields: &mut Vec<String>, items: &Vec<u8>, name: &str) {
    fields.push(format!("{}: Add new element", name));
    for item in items {
        fields.push(item.clone().to_string());
    }
}
fn field_struct_helper(fields: &mut Vec<String>, capnp_struct: &CapnpStruct, name: &str) {
    fields.push(format!("{}: Struct init/clone/set", name));
    for field in &capnp_struct.fields {
        match &field.capnp_type {
            CapnpType::Data(items) => field_data_helper(fields, items, field.name.as_str()),
            CapnpType::Struct(capnp_struct) => {
                field_struct_helper(fields, capnp_struct, field.name.as_str())
            }
            CapnpType::List(capnp_types) => {
                field_list_helper(fields, capnp_types, field.name.as_str())
            }
            _ => fields.push(field.clone().to_string()),
        };
    }
}
fn field_list_helper(fields: &mut Vec<String>, capnp_types: &Vec<CapnpType>, name: &str) {
    fields.push(format!("{}: Add new element", name));
    for field in capnp_types {
        match &field {
            CapnpType::Data(items) => field_data_helper(fields, items, ""),
            CapnpType::Struct(capnp_struct) => field_struct_helper(fields, capnp_struct, ""),
            CapnpType::List(capnp_types) => field_list_helper(fields, capnp_types, ""),
            _ => fields.push(field.clone().to_string("".to_string())),
        };
    }
}

impl Tui<'_> {
    fn find_module_config<'a>(
        config: &'a keystone_config::Reader<'a>,
        instance: &Keystone,
        id: u64,
    ) -> Result<keystone_config::module_config::Reader<'a, any_pointer>> {
        let modules = config.get_modules()?;
        for s in modules.iter() {
            if instance.get_id(s)? == id {
                return Ok(s);
            }
        }

        Err(Error::ModuleNotFound(id).into())
    }

    async fn key(
        &mut self,
        key: KeyEvent,
        rpc_tx: UnboundedSender<Pin<Box<dyn Future<Output = Result<()>>>>>,
        cancellation_token: &CancellationToken,
    ) -> Result<()> {
        use crossterm::event::KeyCode;
        use crossterm::event::KeyEventKind;
        match key.code {
            Char('q') => {
                if key.kind == KeyEventKind::Press {
                    cancellation_token.cancel();
                }
            }
            KeyCode::Enter if key.kind != KeyEventKind::Release => match self.tab {
                TabPage::Keystone => {
                    if let Some(n) = self.keystone.table_state.selected() {
                        // Rust's b-tree implementation doesn't have an efficient nth lookup: https://internals.rust-lang.org/t/suggestion-btreemap-btreeset-o-log-n-n-th-element-access/9515
                        self.module = self.modules.iter().nth(n).map(|(id, _)| *id);
                        if self.module.is_some() {
                            self.tab = TabPage::Module;
                        }
                    }
                }
                TabPage::Module if self.module.is_some() => {
                    if let Some(m) = self.modules.get_mut(&self.module.unwrap()) {
                        match m.state {
                            ModuleState::NotStarted
                            | ModuleState::Closed
                            | ModuleState::Aborted
                            | ModuleState::StartFailure
                            | ModuleState::CloseFailure
                                if m.selected.is_some() =>
                            {
                                match m.selected.unwrap() {
                                    0 => {
                                        let id = m.id;
                                        let tx = self.log_tx.clone();
                                        let s = Self::find_module_config(
                                            &self.config,
                                            &self.instance,
                                            id,
                                        )?;

                                        if let Err(t) = rpc_tx.send(
                                            self.instance
                                                .init_module(
                                                    id,
                                                    s,
                                                    self.dir,
                                                    self.config.get_cap_table()?,
                                                    m.trace.as_str().to_ascii_lowercase(),
                                                    log_capture(id, tx),
                                                )
                                                .await?,
                                        ) {
                                            tokio::try_join!(
                                                self.instance
                                                    .modules
                                                    .get_mut(&m.id)
                                                    .unwrap()
                                                    .stop(self.instance.timeout),
                                                t.0
                                            )
                                            .expect("Failed to recover from failed module start");
                                        }
                                    }
                                    1 => m.trace = m.rotate_trace(),
                                    _ => (),
                                }
                            }
                            ModuleState::Initialized | ModuleState::Ready
                                if m.selected.is_some() =>
                            {
                                match m.selected.unwrap() {
                                    0 => {
                                        if let Some(x) = self.instance.modules.get_mut(&m.id) {
                                            x.stop(self.instance.timeout).await?;
                                        }
                                    }
                                    1 => {
                                        if let Some(x) = self.instance.modules.get_mut(&m.id) {
                                            x.pause(true).await?;
                                        }
                                    }
                                    2 => {
                                        let id = m.id;
                                        let tx = self.log_tx.clone();
                                        let log_state = m.trace.as_str().to_ascii_lowercase();
                                        if let Some(x) = self.instance.modules.get_mut(&id) {
                                            x.stop(self.instance.timeout).await?;
                                        }
                                        let s = Self::find_module_config(
                                            &self.config,
                                            &self.instance,
                                            id,
                                        )?;
                                        if let Err(t) = rpc_tx.send(
                                            self.instance
                                                .init_module(
                                                    id,
                                                    s,
                                                    self.dir,
                                                    self.config.get_cap_table()?,
                                                    log_state,
                                                    log_capture(id, tx),
                                                )
                                                .await?,
                                        ) {
                                            tokio::try_join!(
                                                self.instance
                                                    .modules
                                                    .get_mut(&m.id)
                                                    .unwrap()
                                                    .stop(self.instance.timeout),
                                                t.0
                                            )
                                            .expect("Failed to recover from failed module start");
                                        }
                                    }
                                    3 => self.tab = TabPage::Interface,
                                    4 => m.trace = m.rotate_trace(),
                                    _ => (),
                                }
                            }
                            ModuleState::Paused if m.selected.is_some() => {
                                match m.selected.unwrap() {
                                    0 => {
                                        if let Some(x) = self.instance.modules.get_mut(&m.id) {
                                            x.stop(self.instance.timeout).await?;
                                        }
                                    }
                                    1 => {
                                        if let Some(x) = self.instance.modules.get_mut(&m.id) {
                                            x.pause(false).await?;
                                        }
                                    }
                                    2 => {
                                        let id = m.id;
                                        let tx = self.log_tx.clone();
                                        let log_state = m.trace.as_str().to_ascii_lowercase();
                                        if let Some(x) = self.instance.modules.get_mut(&id) {
                                            x.stop(self.instance.timeout).await?;
                                        }
                                        let s = Self::find_module_config(
                                            &self.config,
                                            &self.instance,
                                            id,
                                        )?;
                                        if let Err(t) = rpc_tx.send(
                                            self.instance
                                                .init_module(
                                                    id,
                                                    s,
                                                    self.dir,
                                                    self.config.get_cap_table()?,
                                                    log_state,
                                                    log_capture(id, tx),
                                                )
                                                .await?,
                                        ) {
                                            tokio::try_join!(
                                                self.instance
                                                    .modules
                                                    .get_mut(&m.id)
                                                    .unwrap()
                                                    .stop(self.instance.timeout),
                                                t.0
                                            )
                                            .expect("Failed to recover from failed module start");
                                        }
                                    }
                                    3 => self.tab = TabPage::Interface,
                                    4 => m.trace = m.rotate_trace(),
                                    _ => (),
                                }
                            }
                            ModuleState::Closing if m.selected.is_some() => {
                                match m.selected.unwrap() {
                                    0 => {
                                        if let Some(x) = self.instance.modules.get_mut(&m.id) {
                                            x.kill().await;
                                        }
                                    }
                                    1 => m.trace = m.rotate_trace(),
                                    _ => (),
                                }
                            }

                            _ => (),
                        }
                    }
                }
                TabPage::Network => {
                    if self.list_state.selected().is_some() {
                        self.tab = TabPage::Keystone;
                    }
                }
                TabPage::Interface => {
                    if let Some(row) = self.keystone.table_state.selected() {
                        let mut desc = &mut self.instance.cap_functions[row as usize];
                        if let Some(col) = self.keystone.table_state.selected_column() {
                            if col == 0 {
                                if desc.type_id == 0 {
                                    return Ok(());
                                }
                                let client = match &desc.module_or_cap {
                                    ModuleOrCap::ModuleId(id) => self
                                        .instance
                                        .modules
                                        .get(&id)
                                        .as_ref()
                                        .unwrap()
                                        .api
                                        .as_ref()
                                        .unwrap()
                                        .pipeline
                                        .get_api()
                                        .as_cap(),
                                    ModuleOrCap::Cap(c) => c.clone(),
                                };
                                let mut call = client.new_call(desc.type_id, desc.method_id, None);
                                if let Some(params_schema) = desc.params_schema.clone() {
                                    //TODO reget schema every time or it explodes after restart
                                    let mut dyn_param_builder =
                                        call.get().init_dynamic(params_schema.into()).unwrap();
                                    for param in &desc.params {
                                        match param.capnp_type.clone() {
                                            CapnpType::Void => dyn_param_builder
                                                .set_named(
                                                    param.name.as_str(),
                                                    dynamic_value::Reader::Void,
                                                )
                                                .unwrap(),
                                            CapnpType::Bool(b) => {
                                                if let Some(b) = b {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::Bool(b),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::Int8(i) => {
                                                if let Some(i) = i {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::Int8(i),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::Int16(i) => {
                                                if let Some(i) = i {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::Int16(i),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::Int32(i) => {
                                                if let Some(i) = i {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::Int32(i),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::Int64(i) => {
                                                if let Some(i) = i {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::Int64(i),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::UInt8(u) => {
                                                if let Some(u) = u {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::UInt8(u),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::UInt16(u) => {
                                                if let Some(u) = u {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::UInt16(u),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::UInt32(u) => {
                                                if let Some(u) = u {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::UInt32(u),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::UInt64(u) => {
                                                if let Some(u) = u {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::UInt64(u),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::Float32(f) => {
                                                if let Some(f) = f {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::Float32(f),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::Float64(f) => {
                                                if let Some(f) = f {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::Float64(f),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::Text(t) => {
                                                if let Some(t) = t {
                                                    dyn_param_builder
                                                        .set_named(
                                                            param.name.as_str(),
                                                            dynamic_value::Reader::Text(
                                                                t.as_str().into(),
                                                            ),
                                                        )
                                                        .unwrap()
                                                }
                                            }
                                            CapnpType::Data(d) => dyn_param_builder
                                                .set_named(
                                                    param.name.as_str(),
                                                    dynamic_value::Reader::Data(d.as_slice()),
                                                )
                                                .unwrap(),
                                            _ => (), //TODO structs and such
                                        }
                                    }
                                    let response = call.send().promise.await;
                                    if let Err(e) = response {
                                        eprintln!("{}", e.extra); //TODO
                                        return Ok(());
                                    }
                                    let response = response.unwrap();
                                    let Some(res_schema) = desc.results_schema.clone() else {
                                        todo!()
                                    };
                                    let get: crate::proxy::GetPointerReader =
                                        response.get().unwrap().get_as().unwrap();
                                    let res_struct = get.reader.get_struct(None).unwrap();
                                    let dyn_reader =
                                        dynamic_struct::Reader::new(res_struct, res_schema.into());
                                    let fields = dyn_reader.get_schema().get_fields().unwrap();
                                    for (index, field) in fields.iter().enumerate() {
                                        desc.results[index].capnp_type =
                                            dyn_reader.get(field).unwrap().try_into().unwrap();
                                    }
                                }
                            } else {
                                if let Some(row) = self.keystone.table_state.selected() {
                                    if let Some(col) = self.keystone.table_state.selected_column() {
                                        if col != 0 && col <= desc.params.len() {
                                            self.input = RowCol { row, col };
                                            self.tab = TabPage::Input;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                TabPage::Input => {
                    if let Some(index) = self.list_state.selected() {
                        let mut index = index as usize;
                        if self.input.col != 0 {
                            let mut desc = &mut self.instance.cap_functions[self.input.row];
                            let mut param = &mut desc.params[self.input.col - 1];
                            match &mut param.capnp_type {
                                CapnpType::Data(items) => {
                                    if index == 0 {
                                        items.push(0);
                                    }
                                }
                                CapnpType::Struct(st) => {
                                    let mut count = 0;
                                    for field in st.fields.iter_mut() {
                                        match &mut field.capnp_type {
                                            CapnpType::Data(items) => {
                                                //if index == 0 {
                                                //    items.push(0);
                                                //}
                                            }
                                            CapnpType::Struct(capnp_struct) => {
                                                for f in capnp_struct.fields.iter_mut() {
                                                    /*if count == index {
                                                        if let CapnpType::Struct(st) = &mut f.capnp_type {
                                                            if st.fields.is_empty() {
                                                                for f in st.schema.get_fields().unwrap() {
                                                                    st.fields.push(ParamResultType {
                                                                        name: f
                                                                            .get_proto()
                                                                            .get_name()
                                                                            .unwrap()
                                                                            .to_string()
                                                                            .unwrap(),
                                                                        capnp_type: f.get_type().which().into(),
                                                                    });
                                                                }
                                                            }
                                                        }
                                                    }*/
                                                }
                                            }
                                            CapnpType::List(items) => {
                                                todo!()
                                            }
                                            _ => (),
                                        }
                                    }
                                }
                                //CapnpType::List(capnp_types) => ,
                                _ => (),
                            }
                        }
                    }
                }
                _ => (),
            },
            KeyCode::Esc if key.kind != KeyEventKind::Release => {
                self.tab = match self.tab {
                    TabPage::Module => TabPage::Keystone,
                    TabPage::Interface => TabPage::Module,
                    TabPage::Network => TabPage::Keystone,
                    _ => self.tab,
                };
            }
            KeyCode::Left if key.kind != KeyEventKind::Release => match self.tab {
                TabPage::Module if self.module.is_some() => {
                    if let Some(x) = self.module.as_ref() {
                        if let Some(m) = self.modules.get_mut(x) {
                            m.selected = if let Some(x) = m.selected {
                                x.checked_sub(1)
                            } else {
                                None
                            };
                        }
                    }
                }
                TabPage::Interface => {
                    self.buffer = String::new();
                    self.keystone.table_state.select_previous_column();
                }
                _ => (),
            },
            KeyCode::Right if key.kind != KeyEventKind::Release => match self.tab {
                TabPage::Module if self.module.is_some() => {
                    if let Some(x) = self.module.as_ref() {
                        if let Some(m) = self.modules.get_mut(x) {
                            m.selected = m.selected.map(|x| x + 1).or(Some(0));
                            m.selected = Some(std::cmp::min(
                                m.selected.unwrap(),
                                match m.state {
                                    ModuleState::NotStarted
                                    | ModuleState::Closed
                                    | ModuleState::Aborted
                                    | ModuleState::StartFailure
                                    | ModuleState::CloseFailure
                                    | ModuleState::Closing => 1,
                                    ModuleState::Initialized
                                    | ModuleState::Ready
                                    | ModuleState::Paused => 4,
                                },
                            ))
                        }
                    }
                }
                TabPage::Interface => {
                    self.buffer = String::new();
                    self.keystone.table_state.select_next_column();
                }
                _ => (),
            },
            KeyCode::Up if key.kind != KeyEventKind::Release => match self.tab {
                TabPage::Keystone => {
                    self.keystone.table_state.select_next();
                }
                TabPage::Network => {
                    self.list_state.select_previous();
                }
                TabPage::Interface => {
                    self.buffer = String::new();
                    self.keystone.table_state.select_previous();
                }
                TabPage::Input => {
                    self.buffer = String::new();
                    self.list_state.select_previous();
                }
                _ => (),
            },
            KeyCode::Down if key.kind != KeyEventKind::Release => match self.tab {
                TabPage::Keystone => {
                    self.keystone.table_state.select_next();
                }
                TabPage::Network => {
                    self.list_state.select_next();
                }
                TabPage::Interface => {
                    self.buffer = String::new();
                    self.keystone.table_state.select_next();
                }
                TabPage::Input => {
                    self.buffer = String::new();
                    self.list_state.select_next();
                }
                _ => (),
            },
            KeyCode::Tab if key.kind != KeyEventKind::Release => {
                let increment = |tab| TABPAGES[((tab as usize) + 1) % TABPAGES.len()];
                self.tab = increment(self.tab);
                if self.tab == TabPage::Module && self.module.is_none() {
                    self.tab = increment(self.tab);
                }
            }
            //TODO idk which keys make sense for this
            KeyCode::Char('-') if key.kind != KeyEventKind::Release => match self.tab {
                TabPage::Interface => {
                    if let Some(row) = self.keystone.table_state.selected() {
                        let desc = &mut self.instance.cap_functions[row];
                        if let Some(col) = self.keystone.table_state.selected_column() {
                            if col != 0 && col <= desc.params.len() {
                                self.holding.push(desc.params[col - 1].clone());
                            } else if col < (desc.params.len() + desc.results.len() + 2) {
                                self.holding
                                    .push(desc.results[col - desc.params.len() - 2].clone());
                            }
                        }
                    }
                }
                _ => (),
            },
            KeyCode::Char('=') if key.kind != KeyEventKind::Release => match self.tab {
                TabPage::Interface => {
                    if let Some(row) = self.keystone.table_state.selected() {
                        let mut desc = &mut self.instance.cap_functions[row];
                        if let Some(col) = self.keystone.table_state.selected_column() {
                            if col != 0 && col <= desc.params.len() {
                                if let Some(p) = self.holding.pop() {
                                    desc.params[col - 1] = p;
                                }
                            }
                        }
                    }
                }
                _ => (),
            },
            KeyCode::Char(c) if key.kind != KeyEventKind::Release => match self.tab {
                TabPage::Interface => {
                    if let Some(row) = self.keystone.table_state.selected() {
                        let mut desc = &mut self.instance.cap_functions[row];
                        if let Some(col) = self.keystone.table_state.selected_column() {
                            if col != 0 && col <= desc.params.len() {
                                self.buffer.push(c);
                                //TODO maybe show an error somewhere
                                match &desc.params[col - 1].capnp_type {
                                    CapnpType::Void => {
                                        todo!();
                                    }
                                    CapnpType::Bool(_) => {
                                        if let Ok(t) = self.buffer.parse::<bool>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Bool(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type = CapnpType::Bool(None);
                                        }
                                    }
                                    CapnpType::Int8(_) => {
                                        if let Ok(t) = self.buffer.parse::<i8>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Int8(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type = CapnpType::Int8(None);
                                        }
                                    }
                                    CapnpType::Int16(_) => {
                                        if let Ok(t) = self.buffer.parse::<i16>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Int16(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Int16(None);
                                        }
                                    }
                                    CapnpType::Int32(_) => {
                                        if let Ok(t) = self.buffer.parse::<i32>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Int32(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Int32(None);
                                        }
                                    }
                                    CapnpType::Int64(_) => {
                                        if let Ok(t) = self.buffer.parse::<i64>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Int64(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Int64(None);
                                        }
                                    }
                                    CapnpType::UInt8(_) => {
                                        if let Ok(t) = self.buffer.parse::<u8>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::UInt8(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::UInt8(None);
                                        }
                                    }
                                    CapnpType::UInt16(_) => {
                                        if let Ok(t) = self.buffer.parse::<u16>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::UInt16(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::UInt16(None);
                                        }
                                    }
                                    CapnpType::UInt32(_) => {
                                        if let Ok(t) = self.buffer.parse::<u32>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::UInt32(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::UInt32(None);
                                        }
                                    }
                                    CapnpType::UInt64(_) => {
                                        if let Ok(t) = self.buffer.parse::<u64>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::UInt64(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::UInt64(None);
                                        }
                                    }
                                    CapnpType::Float32(_) => {
                                        if let Ok(t) = self.buffer.parse::<f32>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Float32(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Float32(None);
                                        }
                                    }
                                    CapnpType::Float64(_) => {
                                        if let Ok(t) = self.buffer.parse::<f64>() {
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Float64(Some(t));
                                        } else {
                                            self.buffer = String::new();
                                            desc.params[col - 1].capnp_type =
                                                CapnpType::Float64(None);
                                        }
                                    }
                                    CapnpType::Text(_) => {
                                        desc.params[col - 1].capnp_type =
                                            CapnpType::Text(Some(self.buffer.clone()));
                                    }
                                    CapnpType::Data(_) => {
                                        todo!();
                                    }
                                    CapnpType::Struct(hash_map) => {
                                        todo!();
                                    }
                                    CapnpType::List(capnp_types) => {
                                        todo!();
                                    }
                                    CapnpType::Capability(client_hook) => {
                                        todo!();
                                    }
                                }
                            }
                        }
                    }
                }
                TabPage::Input => {
                    if let Some(index) = self.list_state.selected() {
                        let mut index = index as usize;
                        if self.input.col != 0 {
                            let mut desc = &mut self.instance.cap_functions[self.input.row];
                            let mut param = &mut desc.params[self.input.col - 1];
                            let mut count = 0;
                            self.buffer.push(c);
                            match &mut param.capnp_type {
                                CapnpType::Data(items) => {
                                    if index != 0 {
                                        if let Ok(t) = self.buffer.parse::<u8>() {
                                            items[index - 1] = t;
                                        } else {
                                            self.buffer = String::new();
                                            items[index - 1] = 0;
                                        }
                                    }
                                }
                                CapnpType::Struct(st) => {
                                    for field in st.fields.iter_mut() {
                                        match &mut field.capnp_type {
                                            CapnpType::Data(items) => {
                                                for item in items {
                                                    if count == index {}
                                                }
                                            }
                                            CapnpType::Struct(capnp_struct) => {
                                                for f in capnp_struct.fields.iter_mut() {}
                                            }
                                            CapnpType::List(items) => {
                                                todo!()
                                            }
                                            _ => (),
                                        }
                                    }
                                }
                                CapnpType::List(l) => {}
                                _ => (),
                            }
                        }
                    }
                }
                _ => (),
            },
            _ => (),
        }
        Ok(())
    }
}

async fn event_loop<B: ratatui::prelude::Backend>(
    instance: &mut Keystone,
    mut terminal: ratatui::Terminal<B>,
    mut event_rx: UnboundedReceiver<TerminalEvent>,
    rpc_tx: UnboundedSender<Pin<Box<dyn Future<Output = Result<()>>>>>,
    mut log_rx: UnboundedReceiver<(u64, String)>,
    log_tx: UnboundedSender<(u64, String)>,
    cancellation_token: CancellationToken,
    dir: &Path,
    config: keystone_config::Reader<'_>,
) -> Result<()> {
    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

    let count = instance.modules.len() as u32;
    let mut app = Tui {
        tab: TabPage::Keystone,
        keystone: TuiKeystone {
            name: gethostname::gethostname().to_string_lossy().to_string(),
            cpu: CircularBuffer::new(),
            ram: CircularBuffer::new(),
            links: 0,
            modules: count,
            loaded: 0,
            state: ModuleState::NotStarted,
            table_state: TableState::default(),
        },
        module: None,
        network: Vec::new(),
        modules: BTreeMap::new(),
        instance,
        dir,
        config,
        list_state: ListState::default(),
        log_tx,
        holding: Vec::new(),
        input: RowCol { row: 0, col: 0 },
        buffer: String::new(),
    };

    let mut sys =
        System::new_with_specifics(RefreshKind::new().with_cpu(CpuRefreshKind::everything()));

    while let Some(evt) = event_rx.recv().await {
        match evt {
            TerminalEvent::Evt(e) => match e {
                Event::Key(key) => app.key(key, rpc_tx.clone(), &cancellation_token).await?,
                Event::Mouse(mouse) => (),
                Event::FocusGained => (),
                Event::FocusLost => (),
                Event::Paste(_) => (),
                Event::Resize(_, _) => (),
            },
            TerminalEvent::Render => terminal
                .draw(|frame| frame.render_widget(&mut app, frame.area()))
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
                let remove = app
                    .modules
                    .iter()
                    .filter_map(|(id, _)| {
                        if app.instance.modules.contains_key(id) {
                            None
                        } else {
                            Some(*id)
                        }
                    })
                    .collect::<Vec<_>>();

                for id in remove {
                    app.modules.remove(&id);
                }

                for (id, m) in &mut app.instance.modules {
                    match m.state {
                        ModuleState::Ready | ModuleState::Paused => app.keystone.loaded += 1,
                        ModuleState::Initialized
                            if app.keystone.state == ModuleState::NotStarted =>
                        {
                            app.keystone.state = ModuleState::Initialized
                        }
                        _ => (),
                    }
                    if let Some(module) = app.modules.get_mut(id) {
                        module.name = m.name.clone();
                        module.state = m.state;
                    } else {
                        //for (k, v) in instance.cap_functions {
                        //root_functions.push(k.to_string());
                        //app.
                        //}
                        app.modules.insert(
                            *id,
                            TuiModule {
                                id: *id,
                                cpu: CircularBuffer::new(),
                                ram: CircularBuffer::new(),
                                name: m.name.clone(),
                                state: m.state,
                                selected: None,
                                trace: tracing::Level::WARN,
                                log: CircularBuffer::new(),
                            },
                        );
                    }
                }
                if let Ok((id, line)) = log_rx.try_recv() {
                    if let Some(m) = app.modules.get_mut(&id) {
                        m.log.push_front(line);
                    }
                }

                if app.keystone.loaded == app.keystone.modules {
                    app.keystone.state = ModuleState::Ready;
                }
            }
        }
    }
    //instance.modules.iter().next().as_ref().unwrap().1.api.as_ref().unwrap().pipeline.get_api().as_cap().new_call(0xbe95_f484_53a4_3d37, 0, None).send().promise.await.unwrap();
    Ok(())
}

async fn event_stream(
    rpc_systems: &mut RpcSystemSet,
    event_tx: UnboundedSender<TerminalEvent>,
    mut rpc_rx: UnboundedReceiver<Pin<Box<dyn Future<Output = Result<()>>>>>,
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
            Some(r) = rpc_systems.next() => if let Err(e) = r {
                eprintln!("Module crashed! {}", e);
            },
            r = rpc_rx.recv() => if let Some(v) = r {
                rpc_systems.push(v);
            }
        }
    }

    Ok(())
}

fn log_capture(id: u64, log_tx: UnboundedSender<(u64, String)>) -> impl LogCapture {
    move |mut input: ByteStreamBufferImpl| {
        let send = log_tx.clone();
        async move {
            let mut buf = [0u8; 1024];
            loop {
                let mut line = String::new();
                loop {
                    let len = input.read(&mut buf).await?;

                    if len == 0 {
                        break;
                    }

                    // If we find a newline, write up to the newline, shift the buffer over, then break
                    if let Some(n) = memchr::memchr(b'\n', &buf[..len]) {
                        if let Ok(s) = std::str::from_utf8(&buf[..n]) {
                            line += s;
                        }
                        buf.copy_within(n..len, 0);
                        break;
                    } else if let Ok(s) = std::str::from_utf8(&buf[..len]) {
                        line += s;
                    }
                }
                // This IS the log function so we can't really do anything if this errors
                if line.is_empty() {
                    break;
                }
                let _ = send.send((id, line));
            }
            Ok::<(), eyre::Report>(())
        }
    }
}

async fn run_interface(
    instance: &mut Keystone,
    rpc_systems: &mut keystone::RpcSystemSet,
    dir: &Path,
    config: keystone_config::Reader<'_>,
    log_rx: UnboundedReceiver<(u64, String)>,
    log_tx: UnboundedSender<(u64, String)>,
) -> Result<()> {
    let mut terminal = ratatui::init();
    terminal.clear()?;

    let cancellation_token = CancellationToken::new();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<TerminalEvent>();

    let (rpc_tx, rpc_rx) = mpsc::unbounded_channel::<Pin<Box<dyn Future<Output = Result<()>>>>>();

    // Catch the error so we always restore the terminal state no matter what happens
    let e = tokio::try_join!(
        event_stream(rpc_systems, event_tx, rpc_rx, cancellation_token.clone()),
        event_loop(
            instance,
            terminal,
            event_rx,
            rpc_tx,
            log_rx,
            log_tx,
            cancellation_token.clone(),
            dir,
            config
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
