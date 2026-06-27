//! Minimal full-screen TUI — `aello` with no args lands here.
//!
//! Browse blueprints, add, edit, delete, self-update, quit. Built on ratatui +
//! crossterm (cross-platform).
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
use ratatui::widgets::{Block, Borders, Cell, Clear, Padding, Paragraph, Row, Table, TableState, Wrap};
use ratatui::{backend::CrosstermBackend, Frame, Terminal};
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};

use crate::models::{Blueprint, Capabilities};
use crate::{config, docs, project, sessions};

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
const GREEN: Color = Color::Rgb(0x4a, 0xff, 0x8a); // success ("matrix" green)

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

/// Global-persona choices for the add flow. Index 0 = none; the rest are
/// built-in templates (kept in sync with `templates::BUILTINS`).
const PERSONAS: &[(&str, &str)] = &[
    ("none", "no global persona"),
    ("coder", "coding agent"),
    ("sysadmin", "ops / devops"),
];

/// Capability checklist rows, in toggle order. `toggle`/`enabled` map an index
/// to the matching `Capabilities` field.
const CAP_ROWS: &[(&str, &str)] = &[
    ("project-md", "maintain a project-level CLAUDE.md"),
    ("github", "/sync commits + pushes to GitHub"),
    ("changelog", "keep CHANGELOG.md current"),
    ("docs", "keep docs/ current"),
    ("readme", "keep README.md current"),
];

fn cap_toggle(caps: &mut Capabilities, i: usize) {
    match i {
        0 => caps.project_md = !caps.project_md,
        1 => caps.github = !caps.github,
        2 => caps.changelog = !caps.changelog,
        3 => caps.docs = !caps.docs,
        4 => caps.readme = !caps.readme,
        _ => {}
    }
}

fn cap_enabled(caps: &Capabilities, i: usize) -> bool {
    match i {
        0 => caps.project_md,
        1 => caps.github,
        2 => caps.changelog,
        3 => caps.docs,
        4 => caps.readme,
        _ => false,
    }
}

/// Picker index for a blueprint's model (for edit pre-selection); 0 if the
/// stored model isn't one of the curated aliases (e.g. a full claude-* id).
fn model_index(model: &str) -> usize {
    MODELS.iter().position(|(id, _)| *id == model).unwrap_or(0)
}

/// Picker index for a blueprint's persona: 0 ("none") if unset or not a
/// built-in (e.g. a custom path).
fn persona_index(claude_md: Option<&str>) -> usize {
    match claude_md {
        None => 0,
        Some(p) => PERSONAS.iter().position(|(id, _)| *id == p).unwrap_or(0),
    }
}

enum Mode {
    Normal,
    AddName { buf: String },
    /// `edit` true means we're editing an existing blueprint, not adding one:
    /// the name step is skipped and each step is pre-seeded from the original,
    /// and the final step updates in place instead of pushing a new blueprint.
    AddModel { name: String, sel: usize, edit: bool },
    /// Pick the global persona (none / built-in template).
    AddPersona { name: String, model: String, sel: usize, edit: bool },
    /// Toggle the capabilities, then create/save. `persona` is the chosen template.
    AddCaps { name: String, model: String, persona: Option<String>, sel: usize, caps: Capabilities, edit: bool },
    ConfirmDelete,
    /// Picking a past session to resume for blueprint `name`.
    Sessions { name: String, items: Vec<sessions::Session>, sel: usize },
    /// Folder picker for the unified contextdb path. `new` Some => typing a
    /// new folder name to create under `dir`.
    Config { dir: PathBuf, entries: Vec<String>, sel: usize, new: Option<String> },
    /// Full-screen reader for the bundled `docs/`. `sel` is the current doc,
    /// `scroll` the vertical line offset into it.
    Help { docs: Vec<docs::Doc>, sel: usize, scroll: u16 },
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

/// Indices of blueprints already placed in the current dir (their env dir
/// exists). These are the ones the launch dir is actually "wearing".
fn local_indices(blueprints: &[Blueprint]) -> Vec<usize> {
    let cwd = std::env::current_dir().unwrap_or_default();
    blueprints
        .iter()
        .enumerate()
        .filter(|(_, b)| project::env_dir(&cwd, &b.name).exists())
        .map(|(i, _)| i)
        .collect()
}

/// The visible blueprint indices for the current filter. Show everything when
/// `show_all`, or when nothing is placed here (an empty registry would just be
/// confusing); otherwise show only the local subset.
fn compute_view(show_all: bool, local: &[usize], total: usize) -> Vec<usize> {
    if show_all || local.is_empty() {
        (0..total).collect()
    } else {
        local.to_vec()
    }
}

struct App {
    blueprints: Vec<Blueprint>,
    /// Indices into `blueprints` placed in the cwd (env dir present).
    local: Vec<usize>,
    /// Indices currently visible — what `selected` indexes into. Either the
    /// local subset (default, when any are local) or every blueprint.
    view: Vec<usize>,
    /// false = show only blueprints placed here; true = show all. Toggled with F.
    show_all: bool,
    selected: usize,
    mode: Mode,
    status: String,
    /// Launch directory as "PARENT / CURRENT", uppercased — shown top-right.
    dir: String,
    has_token: bool,
    /// Max scroll offset for the Help reader, computed from the wrapped content
    /// height during draw (the only place the render width is known) and read
    /// back when handling scroll keys so they can't run past the last line.
    help_scroll_max: std::cell::Cell<u16>,
}

impl App {
    fn load() -> Result<Self> {
        let cfg = config::load()?;
        let blueprints = cfg.blueprints;
        let local = local_indices(&blueprints);
        let mut app = Self {
            has_token: cfg.oauth_token.is_some(),
            blueprints,
            local,
            view: Vec::new(),
            show_all: false,
            selected: 0,
            mode: Mode::Normal,
            status: String::new(),
            dir: launch_dir_label(),
            help_scroll_max: std::cell::Cell::new(0),
        };
        app.rebuild_view();
        Ok(app)
    }

    /// Recompute `view` from `show_all`/`local`, clamping `selected`.
    fn rebuild_view(&mut self) {
        self.view = compute_view(self.show_all, &self.local, self.blueprints.len());
        if self.selected >= self.view.len() {
            self.selected = self.view.len().saturating_sub(1);
        }
    }

    /// The currently-highlighted blueprint, if any.
    fn current(&self) -> Option<&Blueprint> {
        self.view.get(self.selected).and_then(|&i| self.blueprints.get(i))
    }

    fn current_name(&self) -> Option<String> {
        self.current().map(|b| b.name.clone())
    }

    /// Flip the filter, keeping the same blueprint highlighted across the toggle.
    fn set_show_all(&mut self, show_all: bool) {
        let prev = self.current_name();
        self.show_all = show_all;
        self.rebuild_view();
        if let Some(name) = prev {
            if let Some(pos) = self.view.iter().position(|&i| self.blueprints[i].name == name) {
                self.selected = pos;
            }
        }
    }

    /// Reload blueprints from disk, recompute the view, and keep the same
    /// blueprint highlighted (by name) when it still exists.
    fn reload(&mut self) -> Result<()> {
        let prev = self.current_name();
        self.blueprints = config::load()?.blueprints;
        self.local = local_indices(&self.blueprints);
        self.rebuild_view();
        if let Some(name) = prev {
            if let Some(pos) = self.view.iter().position(|&i| self.blueprints[i].name == name) {
                self.selected = pos;
            }
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
                    if let Some(&i) = app.view.get(app.selected) {
                        return Ok(PostExit::Run { name: app.blueprints[i].name.clone(), session: None });
                    }
                }
                KeyCode::Char('s') => {
                    if let Some(&i) = app.view.get(app.selected) {
                        let name = app.blueprints[i].name.clone();
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
                KeyCode::Char('?') => {
                    app.status.clear();
                    app.mode = Mode::Help { docs: docs::all(), sel: 0, scroll: 0 };
                }
                KeyCode::Char('f') => {
                    // Only meaningful when filtering is actually hiding something.
                    if app.local.is_empty() {
                        app.status = "NONE PLACED HERE — SHOWING ALL".into();
                    } else {
                        app.set_show_all(!app.show_all);
                        app.status = if app.show_all { "SHOWING ALL".into() } else { "SHOWING PLACED HERE".into() };
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !app.view.is_empty() {
                        app.selected = (app.selected + 1).min(app.view.len() - 1);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.selected = app.selected.saturating_sub(1);
                }
                KeyCode::Char('a') => {
                    app.status.clear();
                    app.mode = Mode::AddName { buf: String::new() };
                }
                KeyCode::Char('e') => {
                    if let Some(&i) = app.view.get(app.selected) {
                        let b = &app.blueprints[i];
                        let name = b.name.clone();
                        let sel = model_index(&b.model);
                        app.status.clear();
                        app.mode = Mode::AddModel { name, sel, edit: true };
                    } else {
                        app.status = "NO BLUEPRINTS TO EDIT".into();
                    }
                }
                KeyCode::Char('d') | KeyCode::Char('x') => {
                    if app.view.is_empty() {
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
                        Ok(()) => app.mode = Mode::AddModel { name, sel: 0, edit: false },
                        Err(e) => app.status = e.to_string().to_uppercase(),
                    }
                }
                _ => {}
            },
            Mode::AddModel { name, sel, edit } => match key.code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Down | KeyCode::Char('j') => *sel = (*sel + 1).min(MODELS.len() - 1),
                KeyCode::Up | KeyCode::Char('k') => *sel = sel.saturating_sub(1),
                KeyCode::Enter => {
                    let edit = *edit;
                    let name = name.clone();
                    let model = MODELS[*sel].0.to_string();
                    // On edit, pre-select the blueprint's current persona.
                    let sel = if edit {
                        let cfg = config::load()?;
                        cfg.find(&name).map_or(0, |b| persona_index(b.claude_md.as_deref()))
                    } else {
                        0
                    };
                    app.mode = Mode::AddPersona { name, model, sel, edit };
                }
                _ => {}
            },
            Mode::AddPersona { name, model, sel, edit } => match key.code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Down | KeyCode::Char('j') => *sel = (*sel + 1).min(PERSONAS.len() - 1),
                KeyCode::Up | KeyCode::Char('k') => *sel = sel.saturating_sub(1),
                KeyCode::Enter => {
                    let edit = *edit;
                    let name = name.clone();
                    let model = model.clone();
                    // Index 0 is "none"; others are built-in template names.
                    let persona = (*sel != 0).then(|| PERSONAS[*sel].0.to_string());
                    // On edit, start from the blueprint's current capabilities.
                    let caps = if edit {
                        let cfg = config::load()?;
                        cfg.find(&name).map(|b| b.caps.clone()).unwrap_or_default()
                    } else {
                        Capabilities::default()
                    };
                    app.mode = Mode::AddCaps { name, model, persona, sel: 0, caps, edit };
                }
                _ => {}
            },
            Mode::AddCaps { name, model, persona, sel, caps, edit } => match key.code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Down | KeyCode::Char('j') => *sel = (*sel + 1).min(CAP_ROWS.len() - 1),
                KeyCode::Up | KeyCode::Char('k') => *sel = sel.saturating_sub(1),
                KeyCode::Char(' ') => cap_toggle(caps, *sel),
                KeyCode::Enter => {
                    let mut cfg = config::load()?;
                    let mut added: Option<String> = None;
                    if *edit {
                        if let Some(b) = cfg.blueprints.iter_mut().find(|b| b.name == *name) {
                            b.model = model.clone();
                            b.claude_md = persona.clone();
                            b.caps = caps.clone();
                        }
                        config::save(&cfg)?;
                        app.status = format!("UPDATED '{name}'");
                    } else {
                        cfg.blueprints.push(Blueprint {
                            name: name.clone(),
                            model: model.clone(),
                            claude_md: persona.clone(),
                            caps: caps.clone(),
                        });
                        config::save(&cfg)?;
                        app.status = format!("ADDED '{name}'");
                        // A fresh blueprint isn't placed in this dir yet, so the
                        // local filter would hide it — reveal all and select it.
                        added = Some(name.clone());
                    }
                    app.mode = Mode::Normal;
                    if added.is_some() {
                        app.show_all = true;
                    }
                    app.reload()?;
                    if let Some(name) = added {
                        if let Some(pos) = app.view.iter().position(|&i| app.blueprints[i].name == name) {
                            app.selected = pos;
                        }
                    }
                }
                _ => {}
            },
            Mode::ConfirmDelete => match key.code {
                KeyCode::Char('y') => {
                    if let Some(target) = app.current_name() {
                        let mut cfg = config::load()?;
                        cfg.blueprints.retain(|b| b.name != target);
                        config::save(&cfg)?;
                        app.status = format!("REMOVED '{target}'");
                    }
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
            Mode::Help { docs, sel, scroll } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                    app.mode = Mode::Normal;
                }
                KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                    if !docs.is_empty() {
                        *sel = (*sel + 1) % docs.len();
                        *scroll = 0;
                    }
                }
                KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                    if !docs.is_empty() {
                        *sel = (*sel + docs.len() - 1) % docs.len();
                        *scroll = 0;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                KeyCode::Down | KeyCode::Char('j') => {
                    *scroll = (*scroll + 1).min(app.help_scroll_max.get());
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    *scroll = (*scroll + 10).min(app.help_scroll_max.get());
                }
                _ => {}
            },
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
        Mode::AddModel { name, sel, edit } => draw_add_model(f, name, *sel, *edit),
        Mode::AddPersona { name, sel, edit, .. } => draw_add_persona(f, name, *sel, *edit),
        Mode::AddCaps { name, persona, sel, caps, edit, .. } => {
            draw_add_caps(f, name, persona.as_deref(), *sel, caps, *edit)
        }
        Mode::ConfirmDelete => {
            if let Some(b) = app.current() {
                draw_confirm_delete(f, &b.name);
            }
        }
        Mode::Sessions { name, items, sel } => draw_sessions(f, name, items, *sel),
        Mode::Config { dir, entries, sel, new } => draw_config(f, dir, entries, *sel, new),
        Mode::Help { docs, sel, scroll } => draw_help(f, docs, *sel, *scroll, &app.help_scroll_max),
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
    // Left title reflects the filter: PLACED HERE (default subset) vs ALL.
    let filtered = !app.show_all && !app.local.is_empty();
    let scope = if filtered {
        format!(" ▸ PLACED HERE · {} OF {} ", app.view.len(), app.blueprints.len())
    } else {
        format!(" ▸ ALL · {} ", app.blueprints.len())
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title_top(Line::from(Span::styled(scope, Style::default().fg(if filtered { AMBER } else { MUTED }))).left_aligned())
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

    let rows = app.view.iter().enumerate().map(|(row, &i)| {
        let b = &app.blueprints[i];
        let bg = if row % 2 == 0 { SURFACE } else { STRIPE };
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
    // F switches to whichever set isn't shown. With nothing placed here, the
    // filter can't hide anything, so the toggle is shown dimmed.
    let f_label = if app.show_all || app.local.is_empty() { "PLACED" } else { "ALL" };
    let hints = Line::from(vec![
        keyhint("↑/↓", "MOVE"),
        Span::styled(" [↵] RUN  ", Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD)),
        keyhint("F", f_label),
        keyhint("S", "SESSIONS"),
        keyhint("A", "ADD"),
        keyhint("E", "EDIT"),
        keyhint("D", "DELETE"),
        keyhint("C", "CONTEXTDB"),
        keyhint("L", "LOGIN"),
        keyhint("U", "UPDATE"),
        keyhint("?", "DOCS"),
        keyhint("Q", "QUIT"),
    ]);
    let status = Line::from(Span::styled(format!(" {}", app.status), Style::default().fg(ORANGE)));
    let auth_span = if app.has_token {
        Span::styled("AUTH: TOKEN ✓", Style::default().fg(GREEN).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("AUTH: NONE ✗ (press L)", Style::default().fg(ERR))
    };
    let count = if !app.show_all && !app.local.is_empty() {
        format!("{}/{} BLUEPRINT(S)", app.view.len(), app.blueprints.len())
    } else {
        format!("{} BLUEPRINT(S)", app.blueprints.len())
    };
    let telemetry = Line::from(vec![
        Span::styled(
            format!(" AELLO v{VERSION} · {count} · "),
            Style::default().fg(DIM),
        ),
        auth_span,
    ]);
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

fn draw_add_model(f: &mut Frame, name: &str, sel: usize, edit: bool) {
    let h = MODELS.len() as u16 + 6;
    let title = if edit { "EDIT_BLUEPRINT // SELECT_MODEL" } else { "NEW_BLUEPRINT // SELECT_MODEL" };
    let inner = modal(f, title, 56, h);

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
    lines.push(Line::from(Span::styled("  [↑/↓] SELECT · [ENTER] NEXT · [ESC] CANCEL", Style::default().fg(DIM))));

    f.render_widget(Paragraph::new(lines).style(Style::default().bg(SURFACE_HI)), inner);
}

fn draw_add_persona(f: &mut Frame, name: &str, sel: usize, edit: bool) {
    let h = PERSONAS.len() as u16 + 6;
    let title = if edit { "EDIT_BLUEPRINT // GLOBAL_PERSONA" } else { "NEW_BLUEPRINT // GLOBAL_PERSONA" };
    let inner = modal(f, title, 60, h);

    let mut lines = vec![
        Line::from(Span::styled(format!("  NAME = {name}"), Style::default().fg(MUTED))),
        Line::from(""),
    ];
    for (i, (id, desc)) in PERSONAS.iter().enumerate() {
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
    lines.push(Line::from(Span::styled("  [↑/↓] SELECT · [ENTER] NEXT · [ESC] CANCEL", Style::default().fg(DIM))));

    f.render_widget(Paragraph::new(lines).style(Style::default().bg(SURFACE_HI)), inner);
}

fn draw_add_caps(f: &mut Frame, name: &str, persona: Option<&str>, sel: usize, caps: &Capabilities, edit: bool) {
    let h = CAP_ROWS.len() as u16 + 7;
    let title = if edit { "EDIT_BLUEPRINT // SYNC_CAPABILITIES" } else { "NEW_BLUEPRINT // SYNC_CAPABILITIES" };
    let inner = modal(f, title, 64, h);

    let mut lines = vec![
        Line::from(Span::styled(
            format!("  {name} · persona: {}", persona.unwrap_or("none")),
            Style::default().fg(MUTED),
        )),
        Line::from(""),
    ];
    for (i, (id, desc)) in CAP_ROWS.iter().enumerate() {
        let mark = if cap_enabled(caps, i) { "[x]" } else { "[ ]" };
        if i == sel {
            lines.push(Line::from(vec![
                Span::styled(format!(" › {mark} {id} "), Style::default().bg(ORANGE_HOT).fg(Color::Black).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {desc}"), Style::default().fg(AMBER)),
            ]));
        } else {
            let mark_color = if cap_enabled(caps, i) { GREEN } else { DIM };
            lines.push(Line::from(vec![
                Span::styled(format!("   {mark} "), Style::default().fg(mark_color)),
                Span::styled(format!("{id} "), Style::default().fg(TEXT)),
                Span::styled(format!("  {desc}"), Style::default().fg(DIM)),
            ]));
        }
    }
    lines.push(Line::from(""));
    let verb = if edit { "SAVE" } else { "CREATE" };
    lines.push(Line::from(Span::styled(format!("  [SPACE] TOGGLE · [ENTER] {verb} · [ESC] CANCEL"), Style::default().fg(DIM))));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personas_match_builtins() {
        // Picker = "none" + every built-in template, in order.
        let picker: Vec<&str> = PERSONAS.iter().skip(1).map(|(id, _)| *id).collect();
        assert_eq!(picker, crate::templates::BUILTINS);
    }

    #[test]
    fn cap_toggle_round_trips_each_row() {
        for i in 0..CAP_ROWS.len() {
            let mut c = Capabilities::default();
            assert!(!cap_enabled(&c, i));
            cap_toggle(&mut c, i);
            assert!(cap_enabled(&c, i), "row {i} did not toggle on");
        }
    }

    #[test]
    fn markdown_drops_code_fences_and_keeps_lines() {
        // Two fence lines vanish; the code line + the line after remain.
        let lines = render_markdown("```\nfn x() {}\n```\nafter");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn inline_splits_bold_code_and_links() {
        // "a **b** `c` [d](u)" → text, bold, text, code, text, link = 6 spans.
        let spans = inline("a **b** `c` [d](u)");
        assert_eq!(spans.len(), 6);
        // The link renders its label, not the url.
        assert_eq!(spans[5].content.as_ref(), "d");
    }

    #[test]
    fn view_filters_to_local_then_shows_all() {
        // Default (filtered): only the locally-placed subset is visible.
        assert_eq!(compute_view(false, &[1, 3], 5), vec![1, 3]);
        // Toggled to show-all: every blueprint, in order.
        assert_eq!(compute_view(true, &[1, 3], 5), vec![0, 1, 2, 3, 4]);
        // Nothing placed here → fall back to all even when not showing all.
        assert_eq!(compute_view(false, &[], 5), vec![0, 1, 2, 3, 4]);
        // No blueprints at all → empty view, no panic.
        assert_eq!(compute_view(false, &[], 0), Vec::<usize>::new());
    }

    #[test]
    fn bullet_detects_markers() {
        assert_eq!(bullet("- item"), Some("item"));
        assert_eq!(bullet("  * nested"), Some("nested"));
        assert_eq!(bullet("plain"), None);
    }

    #[test]
    fn edit_preselect_indices() {
        // Known aliases / built-ins map to their picker row.
        assert_eq!(model_index("opus"), 0);
        assert_eq!(model_index("haiku"), 2);
        assert_eq!(persona_index(Some("sysadmin")), 2);
        // Unknown values fall back to index 0 (opus / "none").
        assert_eq!(model_index("claude-opus-4-8"), 0);
        assert_eq!(persona_index(None), 0);
        assert_eq!(persona_index(Some("/custom/path.md")), 0);
    }
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

// ── Docs reader ──────────────────────────────────────────────────────────────

/// Full-screen reader for the bundled docs: a list of docs on the left, the
/// selected doc's rendered content (scrollable) on the right.
fn draw_help(
    f: &mut Frame,
    docs: &[docs::Doc],
    sel: usize,
    scroll: u16,
    scroll_max: &std::cell::Cell<u16>,
) {
    let area = f.area();
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ORANGE_HOT))
        .title(Span::styled(
            " DOCS // REFERENCE ",
            Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(
            Line::from(Span::styled(
                " [↑/↓] SCROLL · [TAB/←→] DOC · [ESC] CLOSE ",
                Style::default().fg(DIM),
            ))
            .centered(),
        )
        .style(Style::default().bg(SURFACE));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(20)])
        .split(inner);

    // Left: doc list (titles), current highlighted.
    let list: Vec<Line> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| {
            if i == sel {
                Line::from(Span::styled(
                    format!(" › {} ", d.title),
                    Style::default().bg(ORANGE_HOT).fg(Color::Black).add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(format!("   {}", d.title), Style::default().fg(TEXT)))
            }
        })
        .collect();
    f.render_widget(Paragraph::new(list).style(Style::default().bg(SURFACE_HI)), cols[0]);

    // Right: rendered content, scrolled.
    let content = docs.get(sel).map(|d| render_markdown(d.body)).unwrap_or_default();

    // Cap the scroll at the wrapped content height minus the viewport, so the
    // last line can reach the bottom but you can't scroll into empty space. The
    // paragraph wraps at the text width (pane minus the horizontal padding of 2
    // each side), so a long line occupies several visual rows — counting raw
    // lines (the old cap) stopped short on every wrapped doc.
    let text_w = cols[1].width.saturating_sub(4).max(1) as usize;
    let rows: usize = content
        .iter()
        .map(|l| {
            let w = l.width();
            if w == 0 { 1 } else { w.div_ceil(text_w) }
        })
        .sum();
    let rows = rows.min(u16::MAX as usize) as u16;
    scroll_max.set(rows.saturating_sub(cols[1].height));

    let para = Paragraph::new(content)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false })
        .block(Block::default().padding(Padding::horizontal(2)))
        .style(Style::default().bg(SURFACE));
    f.render_widget(para, cols[1]);
}

/// Render markdown into styled lines for the docs reader. Handles headings,
/// bullets, fenced code blocks, and inline `code`/**bold**/[links]. Not a full
/// markdown engine — just enough to read well in the kinetic style.
fn render_markdown(body: &str) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut in_code = false;
    for raw in body.lines() {
        if raw.trim_start().starts_with("```") {
            in_code = !in_code; // fence toggles a code block; the fence line is dropped
            continue;
        }
        if in_code {
            out.push(Line::from(Span::styled(format!("  {raw}"), Style::default().fg(GREEN))));
        } else if let Some(h) = raw.strip_prefix("### ") {
            out.push(Line::from(Span::styled(h.to_string(), Style::default().fg(AMBER).add_modifier(Modifier::BOLD))));
        } else if let Some(h) = raw.strip_prefix("## ") {
            out.push(Line::from(Span::styled(h.to_uppercase(), Style::default().fg(ORANGE).add_modifier(Modifier::BOLD))));
        } else if let Some(h) = raw.strip_prefix("# ") {
            out.push(Line::from(Span::styled(
                h.to_uppercase(),
                Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )));
        } else if let Some(item) = bullet(raw) {
            let mut spans = vec![Span::styled("  • ", Style::default().fg(AMBER))];
            spans.extend(inline(item));
            out.push(Line::from(spans));
        } else {
            out.push(Line::from(inline(raw)));
        }
    }
    out
}

/// Text after a `- ` / `* ` list marker (leading indent ignored), else None.
fn bullet(line: &str) -> Option<&str> {
    let t = line.trim_start();
    t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))
}

/// Parse a single line of inline markdown into styled spans, handling
/// `**bold**`, `` `code` ``, and `[label](url)` (label only). Everything else
/// is plain text.
fn inline(text: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        // `code`
        if chars[i] == '`' {
            if let Some(end) = find(&chars, i + 1, &['`']) {
                push_text(&mut spans, &mut buf);
                spans.push(Span::styled(slice(&chars, i + 1, end), Style::default().fg(GREEN)));
                i = end + 1;
                continue;
            }
        }
        // **bold**
        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(end) = find(&chars, i + 2, &['*', '*']) {
                push_text(&mut spans, &mut buf);
                spans.push(Span::styled(
                    slice(&chars, i + 2, end),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ));
                i = end + 2;
                continue;
            }
        }
        // [label](url) — render the label, drop the url
        if chars[i] == '[' {
            if let Some(close) = find(&chars, i + 1, &[']']) {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(paren) = find(&chars, close + 2, &[')']) {
                        push_text(&mut spans, &mut buf);
                        spans.push(Span::styled(
                            slice(&chars, i + 1, close),
                            Style::default().fg(AMBER).add_modifier(Modifier::UNDERLINED),
                        ));
                        i = paren + 1;
                        continue;
                    }
                }
            }
        }
        buf.push(chars[i]);
        i += 1;
    }
    push_text(&mut spans, &mut buf);
    if spans.is_empty() {
        spans.push(Span::raw("")); // keep blank lines as real (empty) lines
    }
    spans
}

/// First index >= `from` where `chars` matches `pat`, else None.
fn find(chars: &[char], from: usize, pat: &[char]) -> Option<usize> {
    if pat.is_empty() || from + pat.len() > chars.len() {
        return None;
    }
    (from..=chars.len() - pat.len()).find(|&j| chars[j..j + pat.len()] == *pat)
}

/// Owned String of `chars[start..end]`.
fn slice(chars: &[char], start: usize, end: usize) -> String {
    chars[start..end].iter().collect()
}

/// Flush the plain-text accumulator as a TEXT-styled span.
fn push_text(spans: &mut Vec<Span<'static>>, buf: &mut String) {
    if !buf.is_empty() {
        spans.push(Span::styled(std::mem::take(buf), Style::default().fg(TEXT)));
    }
}
