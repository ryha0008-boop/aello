//! Minimal full-screen TUI — `aello` with no args lands here.
//!
//! Browse blueprints, add, delete, self-update, quit. Run/edit/etc. fill in as
//! later phases land. Built on ratatui + crossterm (cross-platform).
//!
//! Visual style: "Kinetic Command" — inky black, kinetic-orange/amber accents,
//! uppercase monospace labels, sharp bordered modules, telemetry flourishes.

use anyhow::Result;
use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::{backend::CrosstermBackend, Frame, Terminal};
use std::io::{self, Stdout};

use crate::config;
use crate::models::Blueprint;

type Term = Terminal<CrosstermBackend<Stdout>>;

// ── Kinetic Command palette (from DESIGN.md) ────────────────────────────────
const BG: Color = Color::Rgb(0x0a, 0x0a, 0x0a); // inky void
const SURFACE: Color = Color::Rgb(0x14, 0x13, 0x13); // module fill
const STRIPE: Color = Color::Rgb(0x11, 0x11, 0x11); // alternate-row tint
const ORANGE: Color = Color::Rgb(0xff, 0xb5, 0x96); // primary (kinetic orange)
const ORANGE_HOT: Color = Color::Rgb(0xff, 0x66, 0x00); // primary-container
const AMBER: Color = Color::Rgb(0xff, 0xae, 0x00); // secondary (amber glow)
const TEXT: Color = Color::Rgb(0xe5, 0xe2, 0xe1); // on-surface
const MUTED: Color = Color::Rgb(0xaa, 0x8a, 0x7d); // outline
const DIM: Color = Color::Rgb(0x5a, 0x41, 0x36); // outline-variant
const ERR: Color = Color::Rgb(0xff, 0xb4, 0xab); // error

enum Mode {
    Normal,
    /// Adding a blueprint. `name` is None while typing the name, Some once the
    /// name is locked in and the model is being typed. `buf` is the live field.
    Adding { name: Option<String>, buf: String },
    ConfirmDelete,
}

/// What to do after the TUI exits (self-update can't run while the alternate
/// screen is active — defer it until the terminal is restored).
enum PostExit {
    Quit,
    Update,
}

struct App {
    blueprints: Vec<Blueprint>,
    selected: usize,
    mode: Mode,
    status: String,
}

impl App {
    fn load() -> Result<Self> {
        Ok(Self {
            blueprints: config::load()?.blueprints,
            selected: 0,
            mode: Mode::Normal,
            status: String::new(),
        })
    }

    fn reload(&mut self) -> Result<()> {
        self.blueprints = config::load()?.blueprints;
        if self.selected >= self.blueprints.len() {
            self.selected = self.blueprints.len().saturating_sub(1);
        }
        Ok(())
    }
}

pub fn run() -> Result<()> {
    let mut terminal = setup()?;
    let result = run_app(&mut terminal);
    restore(&mut terminal);
    match result? {
        PostExit::Quit => Ok(()),
        PostExit::Update => crate::update::run(),
    }
}

fn setup() -> Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore(terminal: &mut Term) {
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
}

fn run_app(terminal: &mut Term) -> Result<PostExit> {
    let mut app = App::load()?;
    loop {
        terminal.draw(|f| draw(f, &app))?;

        let Event::Key(key) = event::read()? else { continue };
        // Windows emits Press and Release; only act on Press.
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match &mut app.mode {
            Mode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(PostExit::Quit),
                KeyCode::Char('u') => return Ok(PostExit::Update),
                KeyCode::Down | KeyCode::Char('j') => {
                    if !app.blueprints.is_empty() {
                        app.selected = (app.selected + 1).min(app.blueprints.len() - 1);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.selected = app.selected.saturating_sub(1);
                }
                KeyCode::Char('a') => {
                    app.status.clear();
                    app.mode = Mode::Adding { name: None, buf: String::new() };
                }
                KeyCode::Char('d') | KeyCode::Char('x') => {
                    if app.blueprints.is_empty() {
                        app.status = "NO BLUEPRINTS TO DELETE".into();
                    } else {
                        app.mode = Mode::ConfirmDelete;
                    }
                }
                _ => {}
            },
            Mode::Adding { name, buf } => match key.code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) => buf.push(c),
                KeyCode::Enter => {
                    let value = buf.trim().to_string();
                    match name {
                        // Entering the name.
                        None => match crate::validate_name(&value) {
                            Ok(()) if config::load()?.blueprints.iter().any(|b| b.name == value) => {
                                app.status = format!("'{value}' ALREADY EXISTS");
                            }
                            Ok(()) => {
                                app.mode = Mode::Adding { name: Some(value), buf: String::new() };
                            }
                            Err(e) => app.status = e.to_string().to_uppercase(),
                        },
                        // Entering the model.
                        Some(n) => match crate::validate_model(&value) {
                            Ok(()) => {
                                let mut cfg = config::load()?;
                                cfg.blueprints.push(Blueprint {
                                    name: n.clone(),
                                    model: value,
                                    claude_md: None,
                                });
                                config::save(&cfg)?;
                                app.status = format!("ADDED '{n}'");
                                app.mode = Mode::Normal;
                                app.reload()?;
                            }
                            Err(e) => app.status = e.to_string().to_uppercase(),
                        },
                    }
                }
                _ => {}
            },
            Mode::ConfirmDelete => match key.code {
                KeyCode::Char('y') => {
                    let target = app.blueprints[app.selected].name.clone();
                    let mut cfg = config::load()?;
                    cfg.blueprints.retain(|b| b.name != target);
                    config::save(&cfg)?;
                    app.status = format!("REMOVED '{target}'");
                    app.mode = Mode::Normal;
                    app.reload()?;
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                _ => {}
            },
        }
    }
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn draw(f: &mut Frame, app: &App) {
    // Paint the inky-void background across the whole frame.
    f.render_widget(Block::default().style(Style::default().bg(BG)), f.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    draw_header(f, chunks[0]);
    draw_registry(f, chunks[1], app);
    draw_footer(f, chunks[2], app);
}

fn draw_header(f: &mut Frame, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(30)])
        .split(area);

    let brand = Line::from(vec![
        Span::styled(" AELLO", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
        Span::styled("  //  ", Style::default().fg(DIM)),
        Span::styled("BLUEPRINT_REGISTRY", Style::default().fg(MUTED)),
    ]);
    f.render_widget(Paragraph::new(brand).style(Style::default().bg(BG)), cols[0]);

    let telemetry = Line::from(vec![
        Span::styled("SYS_ADMIN_SEC_7 ", Style::default().fg(DIM)),
        Span::styled("◆", Style::default().fg(ORANGE_HOT)),
    ]);
    f.render_widget(
        Paragraph::new(telemetry).alignment(Alignment::Right).style(Style::default().bg(BG)),
        cols[1],
    );
}

fn draw_registry(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(" BLUEPRINTS ", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)))
        .title_top(Line::from(Span::styled(" NODE_REGISTRY·0x7F ", Style::default().fg(DIM))).right_aligned())
        .style(Style::default().bg(SURFACE));

    if app.blueprints.is_empty() {
        let hint = Paragraph::new("\n  NO BLUEPRINTS — PRESS [A] TO ADD")
            .style(Style::default().fg(MUTED).bg(SURFACE))
            .block(block);
        f.render_widget(hint, area);
        return;
    }

    let header = Row::new(["NAME", "MODEL", "CLAUDE.MD", "STATUS"].map(|h| {
        Cell::from(h).style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED))
    }))
    .height(1);

    let rows = app.blueprints.iter().enumerate().map(|(i, b)| {
        let bg = if i % 2 == 0 { SURFACE } else { STRIPE };
        Row::new(vec![
            Cell::from(b.name.clone()).style(Style::default().fg(TEXT)),
            Cell::from(b.model.clone()).style(Style::default().fg(AMBER)),
            Cell::from(b.claude_md.clone().unwrap_or_else(|| "—".into())).style(Style::default().fg(MUTED)),
            Cell::from("● READY").style(Style::default().fg(ORANGE_HOT)),
        ])
        .style(Style::default().bg(bg))
    });

    let table = Table::new(
        rows,
        [Constraint::Length(18), Constraint::Length(16), Constraint::Min(8), Constraint::Length(9)],
    )
    .header(header)
    .block(block)
    .column_spacing(2)
    .row_highlight_style(Style::default().bg(ORANGE_HOT).fg(Color::Black).add_modifier(Modifier::BOLD))
    .highlight_symbol("› ");

    let mut state = TableState::default();
    state.select(Some(app.selected));
    f.render_stateful_widget(table, area, &mut state);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let bg = Style::default().bg(BG);

    // Line 0 — context: key hints (Normal) or the active prompt.
    let context = match &app.mode {
        Mode::Normal => Line::from(vec![
            keyhint("↑/↓", "MOVE"),
            keyhint("A", "ADD"),
            keyhint("D", "DELETE"),
            keyhint("U", "UPDATE"),
            keyhint("Q", "QUIT"),
            Span::styled("RUN:SOON", Style::default().fg(DIM)),
        ]),
        Mode::Adding { name: None, buf } => Line::from(vec![
            Span::styled(" NAME ▸ ", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(buf.clone(), Style::default().fg(TEXT)),
            Span::styled("█", Style::default().fg(ORANGE_HOT)),
            Span::styled("   [ENTER] CONFIRM · [ESC] CANCEL", Style::default().fg(DIM)),
        ]),
        Mode::Adding { name: Some(n), buf } => Line::from(vec![
            Span::styled(format!(" NAME={n}  "), Style::default().fg(MUTED)),
            Span::styled("MODEL ▸ ", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(buf.clone(), Style::default().fg(TEXT)),
            Span::styled("█", Style::default().fg(ORANGE_HOT)),
            Span::styled("   [ENTER] CONFIRM · [ESC] CANCEL", Style::default().fg(DIM)),
        ]),
        Mode::ConfirmDelete => {
            let n = &app.blueprints[app.selected].name;
            Line::from(vec![
                Span::styled(format!(" DELETE '{n}' ? "), Style::default().fg(ERR).add_modifier(Modifier::BOLD)),
                Span::styled("[Y] / [N]", Style::default().fg(MUTED)),
            ])
        }
    };

    // Line 1 — status echo.
    let status = Line::from(Span::styled(
        format!(" {}", app.status),
        Style::default().fg(ORANGE),
    ));

    // Line 2 — telemetry bar.
    let telemetry = Line::from(Span::styled(
        format!(" AELLO v{VERSION} · STABLE · LOCAL_NODE_01 · {} BLUEPRINT(S)", app.blueprints.len()),
        Style::default().fg(DIM),
    ));

    f.render_widget(Paragraph::new(vec![context, status, telemetry]).style(bg), area);
}

/// `[KEY] LABEL ` chip for the footer hint line.
fn keyhint<'a>(key: &'a str, label: &'a str) -> Span<'a> {
    Span::styled(
        format!(" [{key}] {label}  "),
        Style::default().fg(MUTED),
    )
}
