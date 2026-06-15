//! Minimal full-screen TUI — `aello` with no args lands here.
//!
//! Browse blueprints, add, delete, self-update, quit. Run/edit/etc. fill in as
//! later phases land. Built on ratatui + crossterm (cross-platform).
//!
//! Visual style: "Kinetic Command" — inky black, kinetic-orange/amber accents,
//! uppercase monospace labels, sharp bordered modules, centered modal dialogs,
//! telemetry flourishes.

use anyhow::Result;
use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState};
use ratatui::{backend::CrosstermBackend, Frame, Terminal};
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};

use crate::models::Blueprint;
use crate::{config, project, sessions};

type Term = Terminal<CrosstermBackend<Stdout>>;

// ── Kinetic Command palette (from DESIGN.md) ────────────────────────────────
const BG: Color = Color::Rgb(0x0a, 0x0a, 0x0a); // inky void
const SURFACE: Color = Color::Rgb(0x14, 0x13, 0x13); // module fill
const SURFACE_HI: Color = Color::Rgb(0x24, 0x20, 0x1e); // raised bar / modal fill
const STRIPE: Color = Color::Rgb(0x11, 0x11, 0x11); // alternate-row tint
const ORANGE: Color = Color::Rgb(0xff, 0xb5, 0x96); // primary (kinetic orange)
const ORANGE_HOT: Color = Color::Rgb(0xff, 0x66, 0x00); // primary-container
const AMBER: Color = Color::Rgb(0xff, 0xae, 0x00); // secondary (amber glow)
const TEXT: Color = Color::Rgb(0xe5, 0xe2, 0xe1); // on-surface
const MUTED: Color = Color::Rgb(0xaa, 0x8a, 0x7d); // outline
const DIM: Color = Color::Rgb(0x5a, 0x41, 0x36); // outline-variant
const ERR: Color = Color::Rgb(0xff, 0xb4, 0xab); // error

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Launch directory as "PARENT / CURRENT", uppercased (e.g. "WORK / AELLO-TEST").
fn launch_dir_label() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let cur = cwd.file_name().map(|s| s.to_string_lossy().into_owned());
    let parent = cwd.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned());
    match (parent, cur) {
        (Some(p), Some(c)) => format!("{p} / {c}").to_uppercase(),
        (_, Some(c)) => c.to_uppercase(),
        _ => "—".into(),
    }
}

/// Curated model choices — picked from a list so the user never types a model.
const MODELS: &[(&str, &str)] = &[
    ("opus", "most capable"),
    ("sonnet", "balanced speed / intelligence"),
    ("haiku", "fastest, cheapest"),
];

enum Mode {
    Normal,
    AddName { buf: String },
    AddModel { name: String, sel: usize },
    ConfirmDelete,
    /// Picking a past session to resume for blueprint `name`.
    Sessions { name: String, items: Vec<sessions::Session>, sel: usize },
    /// Folder picker for the unified contextdb path. `new` Some => typing a
    /// new folder name to create under `dir`.
    Config { dir: PathBuf, entries: Vec<String>, sel: usize, new: Option<String> },
}

/// Subdirectories of `dir` (sorted, dotfolders hidden), with ".." first if
/// there's a parent.
fn list_dirs(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    v.sort_by_key(|s| s.to_lowercase());
    if dir.parent().is_some() {
        v.insert(0, "..".into());
    }
    v
}

/// Where the folder picker opens: the configured dir if it exists, else its
/// parent, else home, else cwd.
fn browse_start() -> PathBuf {
    let cfg = config::load().unwrap_or_default();
    let resolved = config::contextdb_dir(&cfg);
    if resolved.is_dir() {
        return resolved;
    }
    if let Some(p) = resolved.parent() {
        if p.is_dir() {
            return p.to_path_buf();
        }
    }
    config::home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// What to do after the TUI loop yields. Update/Run need the terminal restored
/// first (Claude takes over the screen); after Run we re-enter the TUI.
enum PostExit {
    Quit,
    Update,
    /// Run `claude setup-token` and store the shared login token.
    Login,
    /// Run a blueprint; `session` Some(id) resumes that session, None starts fresh.
    Run { name: String, session: Option<String> },
}

struct App {
    blueprints: Vec<Blueprint>,
    selected: usize,
    mode: Mode,
    status: String,
    /// Launch directory as "PARENT / CURRENT", uppercased — shown top-right.
    dir: String,
    has_token: bool,
}

impl App {
    fn load() -> Result<Self> {
        let cfg = config::load()?;
        Ok(Self {
            has_token: cfg.oauth_token.is_some(),
            blueprints: cfg.blueprints,
            selected: 0,
            mode: Mode::Normal,
            status: String::new(),
            dir: launch_dir_label(),
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
    // Capture before any update replaces the binary at this path.
    let exe = std::env::current_exe().ok();

    loop {
        let mut terminal = setup()?;
        let result = run_app(&mut terminal);
        restore(&mut terminal);

        match result? {
            PostExit::Quit => return Ok(()),
            PostExit::Update => {
                crate::update::run()?;
                // Re-launch the freshly-installed binary so the TUI reopens on
                // the new version instead of just closing.
                if let Some(exe) = exe {
                    let status = std::process::Command::new(exe).status()?;
                    std::process::exit(status.code().unwrap_or(0));
                }
                return Ok(());
            }
            PostExit::Login => {
                // Terminal restored; setup-token runs its browser flow here.
                match crate::auth::capture_setup_token() {
                    Ok(Some(token)) => {
                        let mut cfg = config::load()?;
                        cfg.oauth_token = Some(token);
                        config::save(&cfg)?;
                        println!("Saved shared login token.");
                    }
                    Ok(None) => println!("Login cancelled."),
                    Err(e) => eprintln!("error: {e:#}"),
                }
                eprintln!("(press Enter to return to aello)");
                let mut _s = String::new();
                let _ = std::io::stdin().read_line(&mut _s);
            }
            PostExit::Run { name, session } => {
                // Terminal is restored; Claude takes over. On return, loop
                // re-enters the TUI fresh. session Some(id) → --resume id.
                let resume = session.map(Some);
                if let Err(e) = crate::run_blueprint(&name, resume, None, &[]) {
                    eprintln!("error: {e:#}");
                    eprintln!("(press Enter to return to aello)");
                    let mut _s = String::new();
                    let _ = std::io::stdin().read_line(&mut _s);
                }
            }
        }
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
        if key.kind != KeyEventKind::Press {
            continue; // Windows emits Press and Release; act on Press only.
        }

        match &mut app.mode {
            Mode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(PostExit::Quit),
                KeyCode::Enter => {
                    if let Some(b) = app.blueprints.get(app.selected) {
                        return Ok(PostExit::Run { name: b.name.clone(), session: None });
                    }
                }
                KeyCode::Char('s') => {
                    if let Some(b) = app.blueprints.get(app.selected) {
                        let name = b.name.clone();
                        let cwd = std::env::current_dir().unwrap_or_default();
                        let env = project::env_dir(&cwd, &name);
                        let items = sessions::list(&env, &cwd);
                        if items.is_empty() {
                            app.status = format!("NO SESSIONS FOR '{name}' IN THIS DIR");
                        } else {
                            app.mode = Mode::Sessions { name, items, sel: 0 };
                        }
                    }
                }
                KeyCode::Char('c') => {
                    app.status.clear();
                    let dir = browse_start();
                    let entries = list_dirs(&dir);
                    app.mode = Mode::Config { dir, entries, sel: 0, new: None };
                }
                KeyCode::Char('l') => return Ok(PostExit::Login),
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
                    app.mode = Mode::AddName { buf: String::new() };
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
            Mode::AddName { buf } => match key.code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) => buf.push(c),
                KeyCode::Enter => {
                    let name = buf.trim().to_string();
                    match crate::validate_name(&name) {
                        Ok(()) if config::load()?.blueprints.iter().any(|b| b.name == name) => {
                            app.status = format!("'{name}' ALREADY EXISTS");
                        }
                        Ok(()) => app.mode = Mode::AddModel { name, sel: 0 },
                        Err(e) => app.status = e.to_string().to_uppercase(),
                    }
                }
                _ => {}
            },
            Mode::AddModel { name, sel } => match key.code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Down | KeyCode::Char('j') => *sel = (*sel + 1).min(MODELS.len() - 1),
                KeyCode::Up | KeyCode::Char('k') => *sel = sel.saturating_sub(1),
                KeyCode::Enter => {
                    let model = MODELS[*sel].0.to_string();
                    let name = name.clone();
                    let mut cfg = config::load()?;
                    cfg.blueprints.push(Blueprint { name: name.clone(), model, claude_md: None });
                    config::save(&cfg)?;
                    app.status = format!("ADDED '{name}'");
                    app.mode = Mode::Normal;
                    app.reload()?;
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
            Mode::Sessions { name, items, sel } => match key.code {
                KeyCode::Esc => app.mode = Mode::Normal,
                KeyCode::Down | KeyCode::Char('j') => *sel = (*sel + 1).min(items.len() - 1),
                KeyCode::Up | KeyCode::Char('k') => *sel = sel.saturating_sub(1),
                KeyCode::Enter => {
                    return Ok(PostExit::Run {
                        name: name.clone(),
                        session: Some(items[*sel].id.clone()),
                    });
                }
                _ => {}
            },
            Mode::Config { dir, entries, sel, new } => {
                if let Some(buf) = new {
                    // Typing a new folder name to create under `dir`.
                    match key.code {
                        KeyCode::Esc => *new = None,
                        KeyCode::Backspace => {
                            buf.pop();
                        }
                        KeyCode::Char(c) => buf.push(c),
                        KeyCode::Enter => {
                            let name = buf.trim();
                            if !name.is_empty() {
                                let target = dir.join(name);
                                if std::fs::create_dir_all(&target).is_ok() {
                                    *dir = target;
                                    *entries = list_dirs(dir);
                                    *sel = 0;
                                }
                            }
                            *new = None;
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Esc => {
                            app.mode = Mode::Normal;
                            app.status = "CANCELLED".into();
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if !entries.is_empty() {
                                *sel = (*sel + 1).min(entries.len() - 1);
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => *sel = sel.saturating_sub(1),
                        KeyCode::Left | KeyCode::Backspace => {
                            if let Some(p) = dir.parent() {
                                *dir = p.to_path_buf();
                                *entries = list_dirs(dir);
                                *sel = 0;
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(name) = entries.get(*sel) {
                                if name == ".." {
                                    if let Some(p) = dir.parent() {
                                        *dir = p.to_path_buf();
                                    }
                                } else {
                                    *dir = dir.join(name);
                                }
                                *entries = list_dirs(dir);
                                *sel = 0;
                            }
                        }
                        KeyCode::Char('n') => *new = Some(String::new()),
                        KeyCode::Char('s') => {
                            let chosen = dir.to_string_lossy().into_owned();
                            let mut cfg = config::load()?;
                            cfg.contextdb = Some(chosen);
                            config::save(&cfg)?;
                            app.status = "CONTEXTDB FOLDER SAVED".into();
                            app.mode = Mode::Normal;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

// ── Rendering ───────────────────────────────────────────────────────────────

fn draw(f: &mut Frame, app: &App) {
    f.render_widget(Block::default().style(Style::default().bg(BG)), f.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    draw_header(f, chunks[0]);
    draw_registry(f, chunks[1], app);
    draw_footer(f, chunks[2], app);

    match &app.mode {
        Mode::Normal => {}
        Mode::AddName { buf } => draw_add_name(f, buf),
        Mode::AddModel { name, sel } => draw_add_model(f, name, *sel),
        Mode::ConfirmDelete => draw_confirm_delete(f, &app.blueprints[app.selected].name),
        Mode::Sessions { name, items, sel } => draw_sessions(f, name, items, *sel),
        Mode::Config { dir, entries, sel, new } => draw_config(f, dir, entries, *sel, new),
    }
}

fn draw_header(f: &mut Frame, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(20)])
        .split(area);

    // Letter-spaced, bold AELLO wordmark — nothing else on the left.
    let brand = Line::from(Span::styled(
        " A E L L O",
        Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD),
    ));
    f.render_widget(Paragraph::new(brand).style(Style::default().bg(BG)), cols[0]);

    let telemetry = Line::from(Span::styled("SYS_ADMIN_SEC_7 ◆ ", Style::default().fg(DIM)));
    f.render_widget(
        Paragraph::new(telemetry).alignment(Alignment::Right).style(Style::default().bg(BG)),
        cols[1],
    );
}

fn draw_registry(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title_top(Line::from(Span::styled(format!(" {} ", app.dir), Style::default().fg(MUTED))).right_aligned())
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
    let hints = Line::from(vec![
        keyhint("↑/↓", "MOVE"),
        Span::styled(" [↵] RUN  ", Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD)),
        keyhint("S", "SESSIONS"),
        keyhint("A", "ADD"),
        keyhint("D", "DELETE"),
        keyhint("C", "CONTEXTDB"),
        keyhint("L", "LOGIN"),
        keyhint("U", "UPDATE"),
        keyhint("Q", "QUIT"),
    ]);
    let status = Line::from(Span::styled(format!(" {}", app.status), Style::default().fg(ORANGE)));
    let auth = if app.has_token { "TOKEN" } else { "PER-ENV LOGIN" };
    let telemetry = Line::from(Span::styled(
        format!(" AELLO v{VERSION} · {} BLUEPRINT(S) · AUTH:{auth}", app.blueprints.len()),
        Style::default().fg(DIM),
    ));
    f.render_widget(
        Paragraph::new(vec![hints, status, telemetry]).style(Style::default().bg(BG)),
        area,
    );
}

/// `[KEY] LABEL` chip for the footer hint line.
fn keyhint<'a>(key: &'a str, label: &'a str) -> Span<'a> {
    Span::styled(format!(" [{key}] {label}  "), Style::default().fg(MUTED))
}

// ── Centered modals ─────────────────────────────────────────────────────────

fn centered(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

/// Bordered modal shell in the kinetic style; returns the inner content area.
fn modal(f: &mut Frame, title: &str, w: u16, h: u16) -> Rect {
    let area = centered(w, h, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ORANGE_HOT))
        .title(Span::styled(format!(" {title} "), Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(SURFACE_HI));
    let inner = block.inner(area);
    f.render_widget(block, area);
    inner
}

fn draw_add_name(f: &mut Frame, buf: &str) {
    let inner = modal(f, "NEW_BLUEPRINT // NAME", 56, 7);
    let body = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  NAME ▸ ", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(buf.to_string(), Style::default().fg(TEXT)),
            Span::styled("█", Style::default().fg(ORANGE_HOT)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  [ENTER] NEXT · [ESC] CANCEL", Style::default().fg(DIM))),
    ];
    f.render_widget(Paragraph::new(body).style(Style::default().bg(SURFACE_HI)), inner);
}

fn draw_add_model(f: &mut Frame, name: &str, sel: usize) {
    let h = MODELS.len() as u16 + 6;
    let inner = modal(f, "NEW_BLUEPRINT // SELECT_MODEL", 56, h);

    let mut lines = vec![
        Line::from(Span::styled(format!("  NAME = {name}"), Style::default().fg(MUTED))),
        Line::from(""),
    ];
    for (i, (id, desc)) in MODELS.iter().enumerate() {
        if i == sel {
            lines.push(Line::from(vec![
                Span::styled(format!(" › {id} "), Style::default().bg(ORANGE_HOT).fg(Color::Black).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {desc}"), Style::default().fg(AMBER)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("   {id} "), Style::default().fg(TEXT)),
                Span::styled(format!("  {desc}"), Style::default().fg(DIM)),
            ]));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  [↑/↓] SELECT · [ENTER] CREATE · [ESC] CANCEL", Style::default().fg(DIM))));

    f.render_widget(Paragraph::new(lines).style(Style::default().bg(SURFACE_HI)), inner);
}

fn draw_confirm_delete(f: &mut Frame, name: &str) {
    let inner = modal(f, "CONFIRM_DELETE", 48, 7);
    let body = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  DELETE  ", Style::default().fg(ERR).add_modifier(Modifier::BOLD)),
            Span::styled(format!("'{name}'"), Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
            Span::styled("  ?", Style::default().fg(ERR).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  [Y] CONFIRM · [N] CANCEL", Style::default().fg(DIM))),
    ];
    f.render_widget(Paragraph::new(body).style(Style::default().bg(SURFACE_HI)), inner);
}

fn draw_config(f: &mut Frame, dir: &Path, entries: &[String], sel: usize, new: &Option<String>) {
    const VIS: usize = 10; // visible rows
    let inner = modal(f, "CONFIG // CONTEXTDB", 72, VIS as u16 + 7);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  DIR ", Style::default().fg(MUTED)),
            Span::styled(dir.display().to_string(), Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
    ];

    if entries.is_empty() {
        lines.push(Line::from(Span::styled("   (no subfolders)", Style::default().fg(DIM))));
    } else {
        // Window the list around the selection.
        let start = sel.saturating_sub(VIS - 1).min(entries.len().saturating_sub(VIS));
        for (i, name) in entries.iter().enumerate().skip(start).take(VIS) {
            let label = if name == ".." { "../".to_string() } else { format!("{name}/") };
            if i == sel {
                lines.push(Line::from(Span::styled(
                    format!(" › {label}"),
                    Style::default().bg(ORANGE_HOT).fg(Color::Black).add_modifier(Modifier::BOLD),
                )));
            } else {
                lines.push(Line::from(Span::styled(format!("   {label}"), Style::default().fg(TEXT))));
            }
        }
    }

    lines.push(Line::from(""));
    if let Some(buf) = new {
        lines.push(Line::from(vec![
            Span::styled("  NEW FOLDER ▸ ", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(buf.clone(), Style::default().fg(TEXT)),
            Span::styled("█", Style::default().fg(ORANGE_HOT)),
        ]));
        lines.push(Line::from(Span::styled("  [↵] CREATE · [ESC] CANCEL", Style::default().fg(DIM))));
    } else {
        // Two short lines so nothing overruns the modal border.
        lines.push(Line::from(Span::styled(
            "  ↑/↓ move · ↵ open · ← up",
            Style::default().fg(DIM),
        )));
        lines.push(Line::from(Span::styled(
            "  [S] select this · [N] new · [ESC] cancel",
            Style::default().fg(DIM),
        )));
    }

    f.render_widget(Paragraph::new(lines).style(Style::default().bg(SURFACE_HI)), inner);
}

fn draw_sessions(f: &mut Frame, name: &str, items: &[sessions::Session], sel: usize) {
    let shown = items.len().min(12);
    let inner = modal(f, &format!("RESUME // {}", name.to_uppercase()), 66, shown as u16 + 5);

    let mut lines = vec![Line::from(Span::styled(
        format!("  {} session(s) — newest first", items.len()),
        Style::default().fg(MUTED),
    ))];
    for (i, s) in items.iter().take(shown).enumerate() {
        let kb = s.size.div_ceil(1024);
        let label = format!("{:<8}  {}  {:>5} KB", &s.id[..s.id.len().min(8)], sessions::format_utc(s.modified), kb);
        if i == sel {
            lines.push(Line::from(Span::styled(
                format!(" › {label}"),
                Style::default().bg(ORANGE_HOT).fg(Color::Black).add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(Span::styled(format!("   {label}"), Style::default().fg(TEXT))));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  [↑/↓] SELECT · [ENTER] RESUME · [ESC] CANCEL",
        Style::default().fg(DIM),
    )));

    f.render_widget(Paragraph::new(lines).style(Style::default().bg(SURFACE_HI)), inner);
}
