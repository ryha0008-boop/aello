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
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
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

use crate::models::{Agent, Blueprint, Role};
use crate::{config, docs, project, sessions, tokens};

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
    ("none", "not a coding project — blank until it earns one"),
    ("coder", "coding agent"),
    ("custom", "this env's own CLAUDE.md"),
];

/// Picker index for a role, so edit mode opens on the current one.
fn role_index(r: Role) -> usize {
    Role::ALL.iter().position(|x| *x == r).unwrap_or(0)
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

/// Resolve an edit-picker value: on edit, keep the blueprint's original value
/// when the user never moved the picker (the curated picker can't represent a
/// full `claude-*` id / `default` / a custom persona path, so writing the
/// highlighted choice would silently downgrade it); otherwise use what they
/// picked. On add (`edit == false`), always use the picked value.
fn resolved_edit<T: Clone>(edit: bool, touched: bool, orig: &T, picked: T) -> T {
    if edit && !touched {
        orig.clone()
    } else {
        picked
    }
}

enum Mode {
    Normal,
    /// First step of the add flow: which CLI this blueprint drives. It comes
    /// before the name because everything after it differs — a Cline blueprint
    /// takes a free-text provider model id where a Claude one takes a curated
    /// alias. Not offered on edit: the two agents share nothing on disk, so
    /// switching one would strand its env rather than convert it.
    AddAgent { sel: usize },
    AddName { buf: String },
    /// Free-text model id for a Cline blueprint. Cline's models are
    /// provider-scoped (`openai/gpt-5.6-luna-pro`, `qwen/qwen3.8-max`) with no
    /// list to pick from, so this is typed rather than chosen.
    AddClineModel { name: String, buf: String, edit: bool },
    /// `edit` true means we're editing an existing blueprint, not adding one:
    /// the name step is skipped and each step is pre-seeded from the original,
    /// and the final step updates in place instead of pushing a new blueprint.
    /// `orig_model` is the blueprint's stored model (may be a full `claude-*`
    /// id / `default` that the curated picker can't represent). On edit, if the
    /// picker ends where it opened, `orig_model` is preserved verbatim instead of
    /// being downgraded to the highlighted curated alias.
    AddModel { name: String, sel: usize, edit: bool, orig_model: String },
    /// Pick the global persona (none / built-in template). `orig_persona` +
    /// and the same end-position comparison preserve a custom persona path.
    AddPersona {
        name: String,
        model: String,
        sel: usize,
        edit: bool,
        orig_persona: Option<String>,
    },
    /// Toggle the capabilities, then create/save. `persona` is the chosen template.
    AddRole { name: String, model: String, persona: Option<String>, sel: usize, edit: bool },
    ConfirmDelete,
    /// Picking a past session to resume for blueprint `name`.
    Sessions { name: String, items: Vec<sessions::Session>, sel: usize },
    /// Folder picker for the unified contextdb path. `new` Some => typing a
    /// new folder name to create under `dir`.
    Config { dir: PathBuf, entries: Vec<String>, sel: usize, new: Option<String> },
    /// Full-screen reader for the bundled `docs/`. `sel` is the current doc,
    /// `scroll` the vertical line offset into it.
    Help { docs: Vec<docs::Doc>, sel: usize, scroll: u16 },
    /// Full-screen token accounting. `sel` is the highlighted env, `scroll` the
    /// offset into its detail pane. The scan itself is cached on `App` — it
    /// reads hundreds of MB of transcripts and takes seconds, which is fine
    /// once and unacceptable per frame.
    Tokens { sel: usize, scroll: u16 },
    /// The charts over the same scan: which projects are hungry, the daily and
    /// hourly shape of the spend, and where the money actually goes.
    TokenStats { scroll: u16 },
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
    local_indices_in(&cwd, blueprints)
}

/// The `cwd`-explicit half, so the agent dispatch is testable without moving the
/// process into a temp directory.
fn local_indices_in(cwd: &std::path::Path, blueprints: &[Blueprint]) -> Vec<usize> {
    blueprints
        .iter()
        .enumerate()
        .filter(|(_, b)| b.agent.env_dir(cwd, &b.name).exists())
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
    /// Which agent the in-progress add flow chose. Carried on `App`
    /// rather than threaded through five Mode variants, which would be a
    /// field on each for one value that never changes mid-flow.
    add_agent: Agent,
    status: String,
    /// Launch directory as "PARENT / CURRENT", uppercased — shown top-right.
    dir: String,
    has_token: bool,
    token_in_vault: bool,
    /// Machine-wide voice mute, cached so the footer doesn't re-read the shared
    /// state file every frame. Refreshed on launch and whenever M toggles it.
    voice_muted: bool,
    /// Max scroll offset for the Help reader, computed from the wrapped content
    /// height during draw (the only place the render width is known) and read
    /// back when handling scroll keys so they can't run past the last line.
    help_scroll_max: std::cell::Cell<u16>,
    /// Cached token scan. `None` until the tab is first opened — the scan walks
    /// every archived transcript (322 MB / ~6s on this machine), so it happens
    /// once per TUI session and on an explicit [R]efresh, never on a redraw.
    tokens: Option<tokens::Report>,
    /// Max scroll for the token detail pane, same deal as `help_scroll_max`.
    tokens_scroll_max: std::cell::Cell<u16>,
}

impl App {
    fn load() -> Result<Self> {
        let cfg = config::load()?;
        let blueprints = cfg.blueprints;
        let local = local_indices(&blueprints);
        let mut app = Self {
            has_token: cfg.oauth_token.is_some(),
            // A token that has moved to the store leaves `config.toml` empty,
            // which the footer used to render as "AUTH: NONE ✗ (press L)" — a
            // false alarm inviting exactly the action that puts the plaintext
            // back. Its own state, not a third meaning bolted onto `has_token`.
            token_in_vault: cfg.oauth_token.is_none() && cfg.vault.is_some(),
            blueprints,
            local,
            view: Vec::new(),
            show_all: false,
            selected: 0,
            mode: Mode::Normal,
            add_agent: Agent::Claude,
            status: String::new(),
            dir: launch_dir_label(),
            voice_muted: crate::voice::is_globally_muted(),
            help_scroll_max: std::cell::Cell::new(0),
            tokens: None,
            tokens_scroll_max: std::cell::Cell::new(0),
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

    // Restore the terminal on a panic anywhere in the draw/event loop — the
    // normal path restores via `restore()`, but a panic would otherwise leave
    // the user in raw mode on the alternate screen with no cursor.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    loop {
        let mut terminal = setup()?;
        let result = run_app(&mut terminal);
        restore(&mut terminal);

        match result? {
            PostExit::Quit => return Ok(()),
            PostExit::Update => {
                crate::update::run(false)?;
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
                    // Through the shared path, never a second copy here: this
                    // branch used to set `cfg.oauth_token` itself, so a TUI
                    // login on a machine whose token had just moved to the store
                    // wrote the plaintext straight back into `config.toml`.
                    Ok(Some(token)) => {
                        if let Err(e) = crate::persist_oauth_token(token) {
                            eprintln!("error: {e:#}");
                        }
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
    // Undo raw mode if entering the alternate screen fails, so we don't leave
    // the user's terminal in raw mode with no TUI running.
    if let Err(e) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(e.into());
    }
    // Terminal::new queries the backend for its size, so it can fail too. Its
    // `?` used to escape run() past the point where restore() is called, leaving
    // the shell in raw mode on the alternate screen with no TUI — and there is
    // no Drop impl anywhere to catch it.
    Terminal::new(CrosstermBackend::new(stdout)).map_err(|e| {
        let mut out = io::stdout();
        let _ = execute!(out, LeaveAlternateScreen);
        let _ = disable_raw_mode();
        e.into()
    })
}

fn restore(terminal: &mut Term) {
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
}

/// A Ctrl or Alt chord. Text entry ignores these rather than inserting the bare
/// letter, which is how Ctrl+C used to type a literal `c` into a blueprint name.
fn is_chord(key: &KeyEvent) -> bool {
    key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// The one chord bound globally, ahead of per-mode dispatch.
fn quits_everywhere(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
}

/// Normalise a key press for command dispatch.
///
/// Two problems, one place. Crossterm delivers Ctrl+<letter> as the lowercase
/// letter plus CONTROL — raw mode means the terminal never intercepts it first —
/// so matching on the code alone ran the plain-letter command: Ctrl+U
/// self-updated without asking, Ctrl+S wrote the contextdb path, Ctrl+D opened
/// the delete modal. And every command arm bound lowercase while the footer
/// advertises `[F] [S] [A] …` and the delete modal says `[Y] CONFIRM`, so
/// Shift+Y or Caps Lock fell through in silence.
///
/// So: a chord on a letter becomes `Null` (matches no arm), and a plain letter
/// folds to lowercase. Non-letter keys — arrows, Enter, Esc — pass through.
fn command_code(key: &KeyEvent) -> KeyCode {
    match key.code {
        KeyCode::Char(_) if is_chord(key) => KeyCode::Null,
        KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
        other => other,
    }
}

fn run_app(terminal: &mut Term) -> Result<PostExit> {
    let mut app = App::load()?;
    loop {
        terminal.draw(|f| draw(f, &app))?;

        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue; // Windows emits Press and Release; act on Press only.
        }

        // Crossterm delivers Ctrl+<letter> as the lowercase letter plus CONTROL,
        // on both platforms, and raw mode means the terminal never intercepts it
        // first. Matching on the code alone therefore ran the plain-letter command:
        // Ctrl+U self-updated without asking, Ctrl+S wrote the contextdb path,
        // Ctrl+D opened the delete modal.
        let chord = is_chord(&key);

        // The most reflexive key there is. Bind it once, ahead of every mode, so
        // it always means the same thing instead of whatever `c` happens to do.
        if quits_everywhere(&key) {
            return Ok(PostExit::Quit);
        }

        // Text-entry modes below deliberately keep `key.code` — a blueprint or
        // folder name needs its original case.
        let code = command_code(&key);

        match &mut app.mode {
            Mode::Normal => match code {
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
                        // Sessions are Claude Code's own transcript files. A
                        // Cline env has none, and reporting "NO SESSIONS" for it
                        // reads as "none yet" rather than "not a thing here".
                        if app.blueprints[i].agent == Agent::Cline {
                            app.status =
                                format!("'{name}' IS A CLINE ENV — NO SESSION HISTORY TO RESUME");
                        } else {
                            let env = project::env_dir(&cwd, &name);
                            let items = sessions::list(&env, &cwd);
                            if items.is_empty() {
                                app.status = format!("NO SESSIONS FOR '{name}' IN THIS DIR");
                            } else {
                                app.mode = Mode::Sessions { name, items, sel: 0 };
                            }
                        }
                    }
                }
                KeyCode::Char('c') => {
                    app.status.clear();
                    let dir = browse_start();
                    let entries = list_dirs(&dir);
                    app.mode = Mode::Config { dir, entries, sel: 0, new: None };
                }
                KeyCode::Char('m') => {
                    // The mute is machine-wide, not per blueprint — the point is
                    // one key that silences everything currently talking at you.
                    match crate::voice::toggle_global_mute() {
                        Ok(muted) => {
                            app.voice_muted = muted;
                            app.status =
                                if muted { "VOICE MUTED (ALL ENVS)" } else { "VOICE UNMUTED" }
                                    .into();
                        }
                        Err(e) => app.status = e.to_string().to_uppercase(),
                    }
                }
                KeyCode::Char('l') => return Ok(PostExit::Login),
                KeyCode::Char('u') => return Ok(PostExit::Update),
                KeyCode::Char('?') => {
                    app.status.clear();
                    app.mode = Mode::Help { docs: docs::all(), sel: 0, scroll: 0 };
                }
                KeyCode::Char('t') => {
                    app.status.clear();
                    app.mode = Mode::Tokens { sel: 0, scroll: 0 };
                    // Paint the SCANNING frame *before* the scan, not after:
                    // reading every archived transcript takes seconds, and
                    // without this the terminal sits on the old screen looking
                    // hung. Cached afterwards — [R] inside the tab rescans.
                    if app.tokens.is_none() {
                        terminal.draw(|f| draw(f, &app))?;
                        let cwd = std::env::current_dir().unwrap_or_default();
                        app.tokens = Some(tokens::scan(&tokens::collect_sources(&cwd)));
                    }
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
                    app.mode = Mode::AddAgent { sel: 0 };
                }
                KeyCode::Char('e') => {
                    if let Some(&i) = app.view.get(app.selected) {
                        let b = &app.blueprints[i];
                        let name = b.name.clone();
                        let sel = model_index(&b.model);
                        let orig_model = b.model.clone();
                        // An agent is fixed at creation, so edit never offers the
                        // picker — it just follows the blueprint's own agent to
                        // the model step that suits it.
                        app.add_agent = b.agent;
                        app.status.clear();
                        app.mode = if b.agent == Agent::Cline {
                            Mode::AddClineModel { name, buf: orig_model, edit: true }
                        } else {
                            Mode::AddModel { name, sel, edit: true, orig_model }
                        };
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
            Mode::AddAgent { sel } => match key.code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *sel = (*sel + 1).min(Agent::ALL.len() - 1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *sel = sel.saturating_sub(1);
                }
                KeyCode::Enter => {
                    app.add_agent = Agent::ALL[*sel];
                    app.mode = Mode::AddName { buf: String::new() };
                }
                _ => {}
            },
            Mode::AddClineModel { name, buf, edit } => match key.code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) if !chord => buf.push(c),
                KeyCode::Enter => {
                    let model = buf.trim().to_string();
                    if model.is_empty() {
                        app.status = "MODEL ID REQUIRED".into();
                    } else {
                        let edit = *edit;
                        let name = name.clone();
                        let (sel, orig_persona) = if edit {
                            let cfg = config::load()?;
                            let b = cfg.find(&name);
                            (
                                b.map_or(0, |b| persona_index(b.claude_md.as_deref())),
                                b.and_then(|b| b.claude_md.clone()),
                            )
                        } else {
                            (0, None)
                        };
                        app.mode = Mode::AddPersona { name, model, sel, edit, orig_persona };
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
                KeyCode::Char(c) if !chord => buf.push(c),
                KeyCode::Enter => {
                    let name = buf.trim().to_string();
                    match crate::validate_name(&name) {
                        Ok(()) if config::load()?.find_name_conflict(&name).is_some() => {
                            app.status = format!("'{name}' ALREADY EXISTS");
                        }
                        Ok(()) if app.add_agent == Agent::Cline => {
                            app.mode =
                                Mode::AddClineModel { name, buf: String::new(), edit: false }
                        }
                        Ok(()) => {
                            app.mode = Mode::AddModel {
                                name,
                                sel: 0,
                                edit: false,
                                orig_model: String::new(),
                            }
                        }
                        Err(e) => app.status = e.to_string().to_uppercase(),
                    }
                }
                _ => {}
            },
            Mode::AddModel { name, sel, edit, orig_model } => match code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *sel = (*sel + 1).min(MODELS.len() - 1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *sel = sel.saturating_sub(1);
                }
                KeyCode::Enter => {
                    let edit = *edit;
                    let name = name.clone();
                    // Compare the final position with where the picker opened,
                    // rather than trusting a one-way latch: browsing down and
                    // back is not a choice to change anything.
                    let moved = *sel != model_index(orig_model);
                    let model = resolved_edit(edit, moved, orig_model, MODELS[*sel].0.to_string());
                    // On edit, pre-select the blueprint's current persona and
                    // remember it so a custom path survives an untouched picker.
                    let (sel, orig_persona) = if edit {
                        let cfg = config::load()?;
                        let b = cfg.find(&name);
                        (
                            b.map_or(0, |b| persona_index(b.claude_md.as_deref())),
                            b.and_then(|b| b.claude_md.clone()),
                        )
                    } else {
                        (0, None)
                    };
                    app.mode = Mode::AddPersona { name, model, sel, edit, orig_persona };
                }
                _ => {}
            },
            Mode::AddPersona { name, model, sel, edit, orig_persona } => match code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *sel = (*sel + 1).min(PERSONAS.len() - 1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *sel = sel.saturating_sub(1);
                }
                KeyCode::Enter => {
                    let edit = *edit;
                    let name = name.clone();
                    let model = model.clone();
                    // Index 0 is "none"; others are built-in template names.
                    let picked = (*sel != 0).then(|| PERSONAS[*sel].0.to_string());
                    let moved = *sel != persona_index(orig_persona.as_deref());
                    let persona = resolved_edit(edit, moved, orig_persona, picked);
                    // On edit, open on the blueprint's current role.
                    let sel = if edit {
                        let cfg = config::load()?;
                        role_index(cfg.find(&name).map(|b| b.role).unwrap_or_default())
                    } else {
                        role_index(Role::Maintainer)
                    };
                    app.mode = Mode::AddRole { name, model, persona, sel, edit };
                }
                _ => {}
            },
            Mode::AddRole { name, model, persona, sel, edit } => match code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "CANCELLED".into();
                }
                KeyCode::Down | KeyCode::Char('j') => *sel = (*sel + 1).min(Role::ALL.len() - 1),
                KeyCode::Up | KeyCode::Char('k') => *sel = sel.saturating_sub(1),
                KeyCode::Enter => {
                    let role = Role::ALL[*sel];
                    let mut cfg = config::load()?;
                    let mut added: Option<String> = None;
    if *edit {
                        // Say so rather than reporting a save that changed
                        // nothing: the blueprint can be removed in another
                        // terminal while these three modals are open.
                        match cfg.blueprints.iter_mut().find(|b| b.name == *name) {
                            Some(b) => {
                                b.model = model.clone();
                                b.claude_md = persona.clone();
                                b.role = role;
                                config::save(&cfg)?;
                                app.status = format!("UPDATED '{name}'");
                            }
                            None => {
                                app.status = format!("'{name}' NO LONGER EXISTS — NOTHING SAVED");
                            }
                        }
                    } else if cfg.find_name_conflict(name).is_some() {
                        // Checked at the name step too, but three interactive
                        // modals separate that check from this write.
                        app.status = format!("'{name}' ALREADY EXISTS — NOTHING SAVED");
                    } else {
                        cfg.blueprints.push(Blueprint {
                            name: name.clone(),
                            model: model.clone(),
                            // Set with `aello edit --mirror-dir`; absent is the in-project mirror.
                            mirror_root: None,
                            agent: app.add_agent,
                            claude_md: persona.clone(),
                            role,
                            legacy_caps: None,
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
            Mode::ConfirmDelete => match code {
                KeyCode::Char('y') => {
                    if let Some(bp) = app.current() {
                        let target = bp.name.clone();
                        // Ask the blueprint which env dir is its own — a Cline
                        // env is `.cline-env-<name>`, and checking Claude's path
                        // for it always came back "nothing left behind" while a
                        // directory holding a plaintext API key sat there.
                        let agent = bp.agent;
                        let mut cfg = config::load()?;
                        cfg.blueprints.retain(|b| b.name != target);
                        config::save(&cfg)?;
                        // The CLI says this; the TUI said nothing, so a deleted
                        // blueprint looked like it took its env dir with it. The
                        // tracked mirror is named too — `aello remove --purge`
                        // clears both, and the CLI's own note omits the mirror.
                        let cwd = std::env::current_dir().unwrap_or_default();
                        let left = agent.env_dir(&cwd, &target).exists();
                        app.status = if left {
                            format!("REMOVED '{target}' — ENV DIR + claude-internal/{target} REMAIN (aello remove {target} --purge)")
                        } else {
                            format!("REMOVED '{target}'")
                        };
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
            Mode::Sessions { name, items, sel } => match code {
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
                        KeyCode::Char(c) if !chord => buf.push(c),
                        KeyCode::Enter => {
                            let name = buf.trim();
                            // Reject what the filesystem will reject, with a
                            // reason. These were accepted and the failure then
                            // swallowed by `.is_ok()`, so the box just closed.
                            const ILLEGAL: &[char] =
                                &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
                            if name.is_empty() {
                                app.status = "FOLDER NAME CANNOT BE EMPTY".into();
                            } else if name.contains(ILLEGAL) {
                                app.status = "FOLDER NAME CONTAINS AN ILLEGAL CHARACTER".into();
                            } else {
                                let target = dir.join(name);
                                match std::fs::create_dir_all(&target) {
                                    Ok(()) => {
                                        *dir = target;
                                        *entries = list_dirs(dir);
                                        *sel = 0;
                                        app.status.clear();
                                    }
                                    Err(e) => app.status = format!("COULD NOT CREATE: {e}"),
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
            Mode::Help { docs, sel, scroll } => match code {
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
                    *scroll = scroll.saturating_add(1).min(app.help_scroll_max.get());
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    *scroll = scroll.saturating_add(10).min(app.help_scroll_max.get());
                }
                // Jump to the ends. Without these, a mis-estimated cap left the
                // tail of a doc permanently out of reach with no way to force it.
                KeyCode::End | KeyCode::Char('g') => *scroll = app.help_scroll_max.get(),
                KeyCode::Home => *scroll = 0,
                _ => {}
            },
            Mode::Tokens { sel, scroll } => {
                let envs = app.tokens.as_ref().map(|r| r.envs.len()).unwrap_or(0);
                match code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('t') => {
                        app.mode = Mode::Normal;
                    }
                    KeyCode::Char('s') => app.mode = Mode::TokenStats { scroll: 0 },
                    KeyCode::Down | KeyCode::Char('j') => {
                        if envs > 0 {
                            *sel = (*sel + 1).min(envs - 1);
                            *scroll = 0;
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        *sel = sel.saturating_sub(1);
                        *scroll = 0;
                    }
                    KeyCode::PageDown | KeyCode::Char(' ') => {
                        *scroll = scroll.saturating_add(10).min(app.tokens_scroll_max.get());
                    }
                    KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                    KeyCode::Right | KeyCode::Char('l') => {
                        *scroll = scroll.saturating_add(1).min(app.tokens_scroll_max.get());
                    }
                    KeyCode::Left | KeyCode::Char('h') => *scroll = scroll.saturating_sub(1),
                    KeyCode::End | KeyCode::Char('g') => *scroll = app.tokens_scroll_max.get(),
                    KeyCode::Home => *scroll = 0,
                    KeyCode::Char('r') => {
                        // Same pre-paint as opening the tab: drop the cache,
                        // show SCANNING, then read the transcripts again.
                        app.tokens = None;
                        terminal.draw(|f| draw(f, &app))?;
                        let cwd = std::env::current_dir().unwrap_or_default();
                        app.tokens = Some(tokens::scan(&tokens::collect_sources(&cwd)));
                    }
                    _ => {}
                }
            }
            Mode::TokenStats { scroll } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('s') => {
                    app.mode = Mode::Tokens { sel: 0, scroll: 0 };
                }
                KeyCode::Char('t') => app.mode = Mode::Normal,
                KeyCode::Down | KeyCode::Char('j') => {
                    *scroll = scroll.saturating_add(1).min(app.tokens_scroll_max.get());
                }
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    *scroll = scroll.saturating_add(10).min(app.tokens_scroll_max.get());
                }
                KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                KeyCode::End | KeyCode::Char('g') => *scroll = app.tokens_scroll_max.get(),
                KeyCode::Home => *scroll = 0,
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
        Mode::AddAgent { sel } => draw_add_agent(f, *sel),
        Mode::AddClineModel { name, buf, edit } => draw_add_cline_model(f, name, buf, *edit),
        Mode::AddName { buf } => draw_add_name(f, buf),
        Mode::AddModel { name, sel, edit, orig_model } => {
            // If editing and the stored model can't be shown in the curated
            // picker (a full id / `default`) and the user hasn't changed it,
            // tell them it will be kept — the highlighted row isn't the truth.
            let keep = (*edit
                && *sel == model_index(orig_model)
                && !MODELS.iter().any(|(id, _)| id == orig_model))
            .then_some(orig_model.as_str());
            draw_add_model(f, name, *sel, *edit, keep);
        }
        Mode::AddPersona { name, sel, edit, orig_persona, .. } => {
            let keep = if *edit && *sel == persona_index(orig_persona.as_deref()) {
                match orig_persona {
                    Some(p) if !PERSONAS.iter().any(|(id, _)| id == p) => Some(p.as_str()),
                    _ => None,
                }
            } else {
                None
            };
            draw_add_persona(f, name, *sel, *edit, keep);
        }
        Mode::AddRole { name, persona, sel, edit, .. } => {
            draw_add_role(f, name, persona.as_deref(), *sel, *edit)
        }
        Mode::ConfirmDelete => {
            if let Some(b) = app.current() {
                draw_confirm_delete(f, &b.name);
            }
        }
        Mode::Sessions { name, items, sel } => draw_sessions(f, name, items, *sel),
        Mode::Config { dir, entries, sel, new } => draw_config(f, dir, entries, *sel, new),
        Mode::Help { docs, sel, scroll } => draw_help(f, docs, *sel, *scroll, &app.help_scroll_max),
        Mode::Tokens { sel, scroll } => draw_tokens(f, app, *sel, *scroll),
        Mode::TokenStats { scroll } => draw_token_stats(f, app, *scroll),
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
        keyhint("M", if app.voice_muted { "UNMUTE" } else { "MUTE" }),
        keyhint("L", "LOGIN"),
        keyhint("U", "UPDATE"),
        keyhint("T", "TOKENS"),
        keyhint("?", "DOCS"),
        keyhint("Q", "QUIT"),
    ]);
    let status = Line::from(Span::styled(format!(" {}", app.status), Style::default().fg(ORANGE)));
    let auth_span = if app.has_token {
        Span::styled("AUTH: TOKEN ✓", Style::default().fg(GREEN).add_modifier(Modifier::BOLD))
    } else if app.token_in_vault {
        Span::styled("AUTH: VAULT ✓", Style::default().fg(GREEN).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("AUTH: NONE ✗ (press L)", Style::default().fg(ERR))
    };
    let count = if !app.show_all && !app.local.is_empty() {
        format!("{}/{} BLUEPRINT(S)", app.view.len(), app.blueprints.len())
    } else {
        format!("{} BLUEPRINT(S)", app.blueprints.len())
    };
    let mut telemetry = vec![
        Span::styled(
            format!(" AELLO v{VERSION} · {count} · "),
            Style::default().fg(DIM),
        ),
        auth_span,
    ];
    // Only surfaced while muted — that's the state you'd otherwise mistake for
    // the voice capability being broken.
    if app.voice_muted {
        telemetry.push(Span::styled(" · VOICE: MUTED", Style::default().fg(AMBER)));
    }
    let telemetry = Line::from(telemetry);
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

/// First step of the add flow. Everything after it differs by agent, which is
/// why it comes first rather than after the name.
fn draw_add_agent(f: &mut Frame, sel: usize) {
    let inner = modal(f, "NEW_BLUEPRINT // AGENT", 64, Agent::ALL.len() as u16 + 6);
    let mut lines = vec![Line::from("")];
    for (i, a) in Agent::ALL.iter().enumerate() {
        let on = i == sel;
        let mark = if on { "▸" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {mark} {:<8}", a.as_str().to_uppercase()),
                Style::default()
                    .fg(if on { ORANGE_HOT } else { TEXT })
                    .add_modifier(if on { Modifier::BOLD } else { Modifier::empty() }),
            ),
            Span::styled(a.describe().to_string(), Style::default().fg(MUTED)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  [↑↓] MOVE · [ENTER] NEXT · [ESC] CANCEL",
        Style::default().fg(DIM),
    )));
    f.render_widget(Paragraph::new(lines).style(Style::default().bg(SURFACE_HI)), inner);
}

/// Cline models are provider-scoped ids with no list to pick from, so this is
/// typed. The curated picker would only ever be wrong here.
fn draw_add_cline_model(f: &mut Frame, name: &str, buf: &str, edit: bool) {
    let title =
        if edit { "EDIT_BLUEPRINT // CLINE_MODEL" } else { "NEW_BLUEPRINT // CLINE_MODEL" };
    let inner = modal(f, title, 64, 9);
    let body = vec![
        Line::from(Span::styled(format!("  NAME = {name}"), Style::default().fg(MUTED))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  MODEL ▸ ", Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)),
            Span::styled(buf.to_string(), Style::default().fg(TEXT)),
            Span::styled("█", Style::default().fg(ORANGE_HOT)),
        ]),
        Line::from(Span::styled(
            "  e.g. openai/gpt-5.6-luna-pro — your provider's own id",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled("  [ENTER] NEXT · [ESC] CANCEL", Style::default().fg(DIM))),
    ];
    f.render_widget(Paragraph::new(body).style(Style::default().bg(SURFACE_HI)), inner);
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

fn draw_add_model(f: &mut Frame, name: &str, sel: usize, edit: bool, keep: Option<&str>) {
    let h = MODELS.len() as u16 + 6 + keep.is_some() as u16;
    let title = if edit { "EDIT_BLUEPRINT // SELECT_MODEL" } else { "NEW_BLUEPRINT // SELECT_MODEL" };
    let inner = modal(f, title, 56, h);

    let mut lines = vec![
        Line::from(Span::styled(format!("  NAME = {name}"), Style::default().fg(MUTED))),
        Line::from(""),
    ];
    if let Some(v) = keep {
        lines.push(Line::from(Span::styled(
            format!("  KEEPING = {v}  ([↑/↓] to change)"),
            Style::default().fg(GREEN),
        )));
    }
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

fn draw_add_persona(f: &mut Frame, name: &str, sel: usize, edit: bool, keep: Option<&str>) {
    let h = PERSONAS.len() as u16 + 6 + keep.is_some() as u16;
    let title = if edit { "EDIT_BLUEPRINT // GLOBAL_PERSONA" } else { "NEW_BLUEPRINT // GLOBAL_PERSONA" };
    let inner = modal(f, title, 60, h);

    let mut lines = vec![
        Line::from(Span::styled(format!("  NAME = {name}"), Style::default().fg(MUTED))),
        Line::from(""),
    ];
    if let Some(v) = keep {
        lines.push(Line::from(Span::styled(
            format!("  KEEPING = {v}  ([↑/↓] to change)"),
            Style::default().fg(GREEN),
        )));
    }
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

fn draw_add_role(f: &mut Frame, name: &str, persona: Option<&str>, sel: usize, edit: bool) {
    let h = Role::ALL.len() as u16 + 7;
    let title = if edit { "EDIT_BLUEPRINT // ROLE" } else { "NEW_BLUEPRINT // ROLE" };
    let inner = modal(f, title, 68, h);

    let mut lines = vec![
        Line::from(Span::styled(
            format!("  {name} · persona: {}", persona.unwrap_or("none")),
            Style::default().fg(MUTED),
        )),
        Line::from(""),
    ];
    for (i, r) in Role::ALL.iter().enumerate() {
        if i == sel {
            lines.push(Line::from(vec![
                Span::styled(format!(" › {:<11} ", r.as_str()), Style::default().bg(ORANGE_HOT).fg(Color::Black).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {}", r.describe()), Style::default().fg(AMBER)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("   {:<11} ", r.as_str()), Style::default().fg(TEXT)),
                Span::styled(format!("  {}", r.describe()), Style::default().fg(DIM)),
            ]));
        }
    }
    lines.push(Line::from(""));
    let verb = if edit { "SAVE" } else { "CREATE" };
    lines.push(Line::from(Span::styled(format!("  [↑/↓] SELECT · [ENTER] {verb} · [ESC] CANCEL"), Style::default().fg(DIM))));

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
        let short: String = s.id.chars().take(8).collect();
        let label = format!("{:<8}  {}  {:>5} KB", short, sessions::format_utc(s.modified), kb);
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
    // ratatui wraps greedily on word boundaries, so a line never occupies FEWER
    // rows than ceil(width / text_w) but often occupies more — a long word that
    // doesn't fit pushes to the next row and leaves the tail of the previous one
    // empty. Estimating from the character count alone therefore under-counted,
    // capped the scroll short, and made the last rows of a doc unreachable
    // (measured: up to 11 rows lost on capabilities.md at 80 columns).
    //
    // Model the greedy break instead: walk the words and start a new row when the
    // next one doesn't fit. Cheap, and exact for the common case.
    let text_w = cols[1].width.saturating_sub(4).max(1) as usize;
    let rows: usize = content
        .iter()
        .map(|l| {
            let text = l.spans.iter().map(|s| s.content.as_ref()).collect::<String>();
            if text.trim().is_empty() {
                return 1;
            }
            let mut rows = 1usize;
            let mut used = 0usize;
            for word in text.split_whitespace() {
                let w = word.chars().count();
                let need = if used == 0 { w } else { w + 1 };
                if used + need > text_w && used > 0 {
                    rows += 1;
                    used = w.min(text_w);
                } else {
                    used += need;
                }
                // A single word longer than the pane wraps hard across rows.
                if w > text_w {
                    rows += (w - 1) / text_w;
                    used = w % text_w;
                }
            }
            rows
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

// ── Tokens tab ──────────────────────────────────────────────────────────────

/// Full-screen token accounting: the 5-hour window across the top, envs down
/// the left, the highlighted env's detail on the right.
fn draw_tokens(f: &mut Frame, app: &App, sel: usize, scroll: u16) {
    let area = f.area();
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ORANGE_HOT))
        .title(Span::styled(
            " TOKENS // USAGE ",
            Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(
            Line::from(Span::styled(
                " [↑/↓] ENV · [PGUP/PGDN] SCROLL · [S] STATS · [R] RESCAN · [ESC] CLOSE ",
                Style::default().fg(DIM),
            ))
            .centered(),
        )
        .style(Style::default().bg(SURFACE));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(report) = app.tokens.as_ref() else {
        // Painted deliberately before the scan runs — see the 't' handler.
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  SCANNING TRANSCRIPTS…",
                    Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "  Reading every archived session in contextdb. A few seconds.",
                    Style::default().fg(MUTED),
                )),
            ])
            .style(Style::default().bg(SURFACE)),
            inner,
        );
        return;
    };

    if report.envs.is_empty() {
        f.render_widget(
            Paragraph::new(format!(
                "\n  NO USAGE RECORDED YET — {} TRANSCRIPT FILE(S) SCANNED",
                report.files_scanned
            ))
            .style(Style::default().fg(MUTED).bg(SURFACE)),
            inner,
        );
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(3)])
        .split(inner);

    draw_token_window(f, rows[0], report);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(24)])
        .split(rows[1]);

    draw_token_envs(f, cols[0], report, sel);
    draw_token_detail(f, cols[1], report, sel, scroll, &app.tokens_scroll_max);
}

/// The current 5-hour rate-limit block, with a bar.
///
/// The bar is scaled against this machine's own peak block — **the
/// subscription quota is in no transcript**, so there is no true denominator to
/// scale against. The label says which ceiling is being used rather than
/// implying a limit aello does not know.
fn draw_token_window(f: &mut Frame, area: Rect, report: &tokens::Report) {
    let now = tokens::now();
    let lines = match report.current_block(now) {
        None => vec![
            Line::from(Span::styled(
                " CURRENT 5H WINDOW",
                Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "   NONE OPEN — NO ACTIVITY IN THE LAST 5 HOURS",
                Style::default().fg(MUTED),
            )),
        ],
        Some(b) => {
            let peak = report.peak_block_tokens();
            let total = b.priced.usage.total();
            let frac = if peak > 0 { (total as f64 / peak as f64).min(1.0) } else { 0.0 };
            let width = 24usize;
            let filled = (frac * width as f64).round() as usize;
            let mut split: Vec<(&String, &tokens::Usage)> = b.by_env.iter().collect();
            split.sort_by_key(|(_, u)| std::cmp::Reverse(u.total()));
            let who: Vec<String> = split
                .iter()
                .take(4)
                .map(|(n, u)| format!("{n} {}", tokens::fmt_tokens(u.total())))
                .collect();
            vec![
                Line::from(vec![
                    Span::styled(
                        " CURRENT 5H WINDOW  ",
                        Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "{} → {}   {} IN · {} LEFT",
                            tokens::fmt_time(b.start),
                            tokens::fmt_time(b.end()),
                            tokens::fmt_duration(now - b.start),
                            tokens::fmt_duration(b.end() - now),
                        ),
                        Style::default().fg(MUTED),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("   ", Style::default()),
                    Span::styled(
                        "█".repeat(filled),
                        Style::default().fg(if frac > 0.8 { ERR } else { ORANGE_HOT }),
                    ),
                    Span::styled("░".repeat(width - filled), Style::default().fg(DIM)),
                    Span::styled(
                        format!(
                            "  {} · {} · {:.0}% OF PEAK BLOCK ({})",
                            tokens::fmt_tokens(total),
                            tokens::fmt_cost(b.priced.cost),
                            frac * 100.0,
                            tokens::fmt_tokens(peak),
                        ),
                        Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(Span::styled(format!("   {}", who.join(" · ")), Style::default().fg(TEXT))),
            ]
        }
    };
    f.render_widget(Paragraph::new(lines).style(Style::default().bg(SURFACE_HI)), area);
}

fn draw_token_envs(f: &mut Frame, area: Rect, report: &tokens::Report, sel: usize) {
    let header = Row::new(["ENV", "TOTAL", "COST"].map(|h| {
        Cell::from(h)
            .style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED))
    }))
    .height(1);

    let rows = report.envs.iter().enumerate().map(|(i, e)| {
        let bg = if i % 2 == 0 { SURFACE } else { STRIPE };
        Row::new(vec![
            Cell::from(e.blueprint.clone()).style(Style::default().fg(TEXT)),
            Cell::from(tokens::fmt_tokens(e.priced.usage.total()))
                .style(Style::default().fg(AMBER)),
            Cell::from(tokens::fmt_cost(e.priced.cost)).style(Style::default().fg(ORANGE_HOT)),
        ])
        .style(Style::default().bg(bg))
    });

    let total = report.total();
    let table = Table::new(
        rows,
        [Constraint::Min(10), Constraint::Length(7), Constraint::Length(9)],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(DIM))
            .title_bottom(Line::from(Span::styled(
                format!(
                    " ALL: {} · {} ",
                    tokens::fmt_tokens(total.usage.total()),
                    tokens::fmt_cost(total.cost)
                ),
                Style::default().fg(MUTED),
            )))
            .style(Style::default().bg(SURFACE)),
    )
    .column_spacing(1)
    .row_highlight_style(
        Style::default().bg(ORANGE_HOT).fg(Color::Black).add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("›");

    let mut state = TableState::default();
    state.select(Some(sel.min(report.envs.len().saturating_sub(1))));
    f.render_stateful_widget(table, area, &mut state);
}

/// Right pane: the highlighted env's buckets, per-model split, and sessions.
fn draw_token_detail(
    f: &mut Frame,
    area: Rect,
    report: &tokens::Report,
    sel: usize,
    scroll: u16,
    scroll_max: &std::cell::Cell<u16>,
) {
    let Some(e) = report.envs.get(sel) else { return };
    let mut lines: Vec<Line> = Vec::new();

    let kv = |k: &str, v: String| {
        Line::from(vec![
            Span::styled(format!("  {k:<14}"), Style::default().fg(MUTED)),
            Span::styled(v, Style::default().fg(TEXT)),
        ])
    };
    let section = |t: &str| {
        Line::from(Span::styled(
            format!(" {t}"),
            Style::default().fg(ORANGE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ))
    };

    let u = &e.priced.usage;
    lines.push(Line::from(Span::styled(
        format!(" {} ", e.blueprint),
        Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD),
    )));
    lines.push(kv("PROJECTS", e.projects.iter().cloned().collect::<Vec<_>>().join(", ")));
    lines.push(kv(
        "ACTIVE",
        format!("{} → {}", tokens::fmt_time(e.first), tokens::fmt_time(e.last)),
    ));
    lines.push(Line::from(""));
    lines.push(section("TOKENS"));
    lines.push(kv("INPUT", tokens::fmt_tokens(u.input)));
    lines.push(kv("OUTPUT", tokens::fmt_tokens(u.output)));
    lines.push(kv(
        "CACHE WRITE",
        format!(
            "{}   (5m {} · 1h {})",
            tokens::fmt_tokens(u.cache_write()),
            tokens::fmt_tokens(u.cache_write_5m),
            tokens::fmt_tokens(u.cache_write_1h)
        ),
    ));
    lines.push(kv("CACHE READ", tokens::fmt_tokens(u.cache_read)));
    lines.push(kv("TOTAL", tokens::fmt_tokens(u.total())));
    lines.push(Line::from(vec![
        Span::styled("  COST          ", Style::default().fg(MUTED)),
        Span::styled(
            tokens::fmt_cost(e.priced.cost),
            Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  (list API rates, estimated)", Style::default().fg(DIM)),
    ]));
    if e.priced.unpriced_tokens > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "  {} TOKEN(S) UNPRICED — NO RATE FOR THAT MODEL",
                tokens::fmt_tokens(e.priced.unpriced_tokens)
            ),
            Style::default().fg(ERR),
        )));
    }

    lines.push(Line::from(""));
    lines.push(section("BY MODEL"));
    let mut models: Vec<(&String, &tokens::Usage)> = e.priced.by_model.iter().collect();
    models.sort_by_key(|(_, u)| std::cmp::Reverse(u.total()));
    for (m, mu) in models {
        lines.push(Line::from(vec![
            Span::styled(format!("  {m:<28}"), Style::default().fg(TEXT)),
            Span::styled(
                format!("{:>8}", tokens::fmt_tokens(mu.total())),
                Style::default().fg(AMBER),
            ),
            Span::styled(
                format!("  {:>9}", mu.cost(m).map(tokens::fmt_cost).unwrap_or_else(|| "—".into())),
                Style::default().fg(MUTED),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(section(&format!("SESSIONS ({})", e.sessions.len())));
    lines.push(Line::from(Span::styled(
        format!("  {:<17}{:>5}{:>8}{:>10}{:>10}", "STARTED", "MSGS", "SPAN", "TOTAL", "COST"),
        Style::default().fg(DIM),
    )));
    for s in &e.sessions {
        lines.push(Line::from(Span::styled(
            format!(
                "  {:<17}{:>5}{:>8}{:>10}{:>10}",
                tokens::fmt_time(s.first),
                s.messages,
                tokens::fmt_duration(s.last - s.first),
                tokens::fmt_tokens(s.priced.usage.total()),
                tokens::fmt_cost(s.priced.cost),
            ),
            Style::default().fg(TEXT),
        )));
    }

    // Same cap discipline as the docs reader: without it the tail of a long
    // session list is unreachable.
    let viewport = area.height.saturating_sub(1);
    scroll_max.set((lines.len() as u16).saturating_sub(viewport));

    f.render_widget(
        Paragraph::new(lines)
            .scroll((scroll, 0))
            .style(Style::default().bg(SURFACE))
            .block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        area,
    );
}

// ── Token statistics page ───────────────────────────────────────────────────

/// How many days the daily chart covers. Wider than a terminal is usually
/// worth; the number is stated on the chart so nobody reads it as "all time".
const CHART_DAYS: i64 = 30;

/// Sparklines live in `tokens` now — the CLI `--stats` view draws them too, and
/// two copies of the "a non-zero day is never blank" rule is one too many.
use tokens::spark;

/// A proportional bar, `width` cells wide.
fn hbar(frac: f64, width: usize) -> String {
    let filled = (frac.clamp(0.0, 1.0) * width as f64).round() as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(width.saturating_sub(filled)))
}

fn draw_token_stats(f: &mut Frame, app: &App, scroll: u16) {
    let area = f.area();
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ORANGE_HOT))
        .title(Span::styled(
            " TOKENS // STATISTICS ",
            Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(
            Line::from(Span::styled(
                " [↑/↓ PGUP/PGDN] SCROLL · [S/ESC] BACK · [T] CLOSE ",
                Style::default().fg(DIM),
            ))
            .centered(),
        )
        .style(Style::default().bg(SURFACE));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(report) = app.tokens.as_ref() else { return };
    let s = tokens::stats(report, CHART_DAYS);
    let total = report.total();

    let section = |t: &str| {
        Line::from(Span::styled(
            format!(" {t}"),
            Style::default().fg(ORANGE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ))
    };
    let note = |t: String| Line::from(Span::styled(format!("  {t}"), Style::default().fg(DIM)));

    let mut lines: Vec<Line> = Vec::new();

    // ── Headline ────────────────────────────────────────────────────────────
    let msgs = report.messages as u64;
    let per_msg = if msgs > 0 { total.cost / msgs as f64 } else { 0.0 };
    let per_sess = if s.sessions > 0 { total.cost / s.sessions as f64 } else { 0.0 };
    lines.push(section("OVERALL"));
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(
            format!("{} tokens", tokens::fmt_tokens(total.usage.total())),
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(DIM)),
        Span::styled(
            tokens::fmt_cost(total.cost),
            Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                " · {} sessions · {} messages · {} → {}",
                s.sessions,
                msgs,
                s.daily.first().map(|d| tokens::fmt_time(d.day)).unwrap_or_else(|| "—".into()),
                s.daily.last().map(|d| tokens::fmt_time(d.day)).unwrap_or_else(|| "—".into()),
            ),
            Style::default().fg(TEXT),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        format!(
            "  {} per day · {} per session · {} per message",
            tokens::fmt_cost(s.cost_per_day(total.cost)),
            tokens::fmt_cost(per_sess),
            tokens::fmt_cost(per_msg),
        ),
        Style::default().fg(MUTED),
    )));
    lines.push(note("list API rates on subscription usage — a what-if, never a bill".into()));
    if report.pointer_only > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "  {} archived session(s) hold only a pointer to a deleted transcript and count as ZERO here",
                report.pointer_only
            ),
            Style::default().fg(ERR),
        )));
    }
    lines.push(note(
        "totals cover contextdb plus the live envs of THIS directory — run from a project to see its unarchived sessions"
            .into(),
    ));
    lines.push(Line::from(""));

    // ── Token-hungry projects ───────────────────────────────────────────────
    lines.push(section("TOKEN-HUNGRY PROJECTS  (tokens ÷ sessions)"));
    lines.push(Line::from(Span::styled(
        format!(
            "  {:<22}{:>6}{:>9}{:>10}{:>9}  {}",
            "PROJECT", "SESS", "TOTAL", "PER SESS", "$/SESS", "RELATIVE"
        ),
        Style::default().fg(DIM),
    )));
    let hungriest = s.projects.first().map(|p| p.per_session()).unwrap_or(0);
    for p in &s.projects {
        let frac = if hungriest > 0 { p.per_session() as f64 / hungriest as f64 } else { 0.0 };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<22}", truncate(&p.project, 21)),
                Style::default().fg(TEXT),
            ),
            Span::styled(format!("{:>6}", p.sessions), Style::default().fg(MUTED)),
            Span::styled(
                format!("{:>9}", tokens::fmt_tokens(p.priced.usage.total())),
                Style::default().fg(MUTED),
            ),
            Span::styled(
                format!("{:>10}", tokens::fmt_tokens(p.per_session())),
                Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:>9}", tokens::fmt_cost(p.cost_per_session())),
                Style::default().fg(ORANGE_HOT),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(hbar(frac, 16), Style::default().fg(ORANGE_HOT)),
        ]));
    }
    lines.push(note(
        "how expensive it is to ENGAGE with a project, not how much it has been used — \
         few long sessions outrank many short ones"
            .into(),
    ));
    lines.push(Line::from(""));

    // ── Where the money goes ────────────────────────────────────────────────
    lines.push(section("WHERE THE MONEY GOES  (token share vs cost share)"));
    let u = &total.usage;
    let tok = u.total().max(1) as f64;
    let cost = s.bucket_cost.total().max(1e-9);
    let buckets: [(&str, u64, f64); 4] = [
        ("input", u.input, s.bucket_cost.input),
        ("output", u.output, s.bucket_cost.output),
        ("cache write", u.cache_write(), s.bucket_cost.cache_write),
        ("cache read", u.cache_read, s.bucket_cost.cache_read),
    ];
    lines.push(Line::from(Span::styled(
        format!("  {:<13}{:>10}{:>8}   {:<20}{:>10}{:>8}", "BUCKET", "TOKENS", "%", "", "COST", "%"),
        Style::default().fg(DIM),
    )));
    for (name, tokens_n, dollars) in buckets {
        let tp = tokens_n as f64 / tok;
        let cp = dollars / cost;
        lines.push(Line::from(vec![
            Span::styled(format!("  {name:<13}"), Style::default().fg(TEXT)),
            Span::styled(
                format!("{:>10}{:>7.1}%", tokens::fmt_tokens(tokens_n), tp * 100.0),
                Style::default().fg(AMBER),
            ),
            Span::styled(format!("  {}", hbar(tp, 10)), Style::default().fg(AMBER)),
            Span::styled(
                format!("  {:>10}{:>7.1}%", tokens::fmt_cost(dollars), cp * 100.0),
                Style::default().fg(ORANGE_HOT),
            ),
            Span::styled(format!("  {}", hbar(cp, 10)), Style::default().fg(ORANGE_HOT)),
        ]));
    }
    lines.push(note(
        "cache is not uniformly cheap: a read is 0.1x input, but a 1h write is 2x it".into(),
    ));
    lines.push(Line::from(""));

    // ── Cost split by branch and by reasoning effort ─────────────────────────
    // Both are one shape, so they share one renderer. A single-row split is
    // dropped: "100% on main" is not a finding.
    let mut slice_table = |title: &str, slices: &[tokens::Slice]| {
        if slices.len() < 2 {
            return;
        }
        lines.push(section(title));
        lines.push(Line::from(Span::styled(
            format!("  {:<24}{:>6}{:>8}{:>10}{:>10}  {}", "", "SESS", "MSGS", "TOKENS", "COST", "SHARE OF COST"),
            Style::default().fg(DIM),
        )));
        let top = slices.first().map(|s| s.priced.cost).unwrap_or(0.0).max(1e-9);
        for s in slices {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<24}", truncate(&s.name, 23)),
                    Style::default().fg(if s.name == tokens::UNRECORDED { MUTED } else { TEXT }),
                ),
                Span::styled(format!("{:>6}{:>8}", s.sessions, s.messages), Style::default().fg(MUTED)),
                Span::styled(
                    format!("{:>10}", tokens::fmt_tokens(s.priced.usage.total())),
                    Style::default().fg(AMBER),
                ),
                Span::styled(
                    format!("{:>10}", tokens::fmt_cost(s.priced.cost)),
                    Style::default().fg(ORANGE_HOT),
                ),
                Span::styled("  ", Style::default()),
                Span::styled(hbar(s.priced.cost / top, 14), Style::default().fg(ORANGE_HOT)),
            ]));
        }
        lines.push(Line::from(""));
    };
    slice_table("SPEND BY BRANCH", &s.branches);
    slice_table("SPEND BY REASONING EFFORT", &s.efforts);
    if s.efforts.iter().any(|e| e.name == tokens::UNRECORDED) {
        lines.push(note(
            "(unrecorded) is its own row on purpose — the field only exists on newer records, and \
             folding it into `high` would invent the number"
                .into(),
        ));
        lines.push(Line::from(""));
    }

    // ── Daily ───────────────────────────────────────────────────────────────
    lines.push(section(&format!("DAILY  (last {} days, UTC)", s.daily.len())));
    let day_tokens: Vec<u64> = s.daily.iter().map(|d| d.tokens).collect();
    let peak = s.busiest_day();
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(spark(&day_tokens), Style::default().fg(ORANGE_HOT)),
        Span::styled(
            format!(
                "  peak {} on {}",
                tokens::fmt_tokens(peak.map(|d| d.tokens).unwrap_or(0)),
                peak.map(|d| tokens::fmt_time(d.day)).unwrap_or_else(|| "—".into()),
            ),
            Style::default().fg(MUTED),
        ),
    ]));
    if let (Some(a), Some(b)) = (s.daily.first(), s.daily.last() ) {
        lines.push(Line::from(Span::styled(
            format!("  {:<width$}{}", tokens::fmt_time(a.day), tokens::fmt_time(b.day),
                width = s.daily.len().saturating_sub(11).max(1)),
            Style::default().fg(DIM),
        )));
    }
    let charted: u64 = day_tokens.iter().sum();
    let charted_cost: f64 = s.daily.iter().map(|d| d.cost).sum();
    lines.push(note(format!(
        "{} · {} in the charted window · gaps are real days with no work",
        tokens::fmt_tokens(charted),
        tokens::fmt_cost(charted_cost)
    )));
    lines.push(Line::from(""));

    // ── Models over time ────────────────────────────────────────────────────
    // One line per model over the same window as DAILY above, so a migration
    // reads as a handover rather than as one blended average.
    if s.model_daily.len() > 1 {
        lines.push(section("MODELS OVER TIME  (same window as DAILY)"));
        for (i, (model, series)) in s.model_daily.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<26}", truncate(model, 25)), Style::default().fg(TEXT)),
                Span::styled(
                    spark(series),
                    Style::default().fg(if i == 0 { ORANGE_HOT } else { AMBER }),
                ),
            ]));
        }
        for (model, first, last) in &s.model_span {
            lines.push(Line::from(Span::styled(
                format!(
                    "  {:<26}{} → {}",
                    truncate(model, 25),
                    tokens::fmt_time(*first),
                    tokens::fmt_time(*last)
                ),
                Style::default().fg(DIM),
            )));
        }
        lines.push(note(
            "the dates are all history; the bars are only the charted window".into(),
        ));
        lines.push(Line::from(""));
    }

    // ── Hour of day ─────────────────────────────────────────────────────────
    lines.push(section("HOUR OF DAY  (UTC — no timezone conversion, so read the offset in)"));
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(spark(&s.hourly), Style::default().fg(AMBER)),
        Span::styled(
            format!(
                "  busiest {:02}:00 UTC",
                s.peak_hour().unwrap_or(0)
            ),
            Style::default().fg(MUTED),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "  00  03  06  09  12  15  18  21".to_string(),
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(""));

    // ── What the sessions did ───────────────────────────────────────────────
    let a = &report.activity;
    if a.turns() > 0 {
        let hours = a.turn_hours();
        lines.push(section("WHAT THE SESSIONS DID"));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{} turns", a.turns()),
                Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " · median {} · p90 {} · longest {}",
                    tokens::fmt_secs(a.median_turn_secs()),
                    tokens::fmt_secs(a.p90_turn_secs()),
                    tokens::fmt_secs(a.longest_turn_secs()),
                ),
                Style::default().fg(TEXT),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(format!("  {hours:.1}h inside turns · "), Style::default().fg(TEXT)),
            Span::styled(
                format!(
                    "{} per hour of it",
                    tokens::fmt_cost(if hours > 0.0 { total.cost / hours } else { 0.0 })
                ),
                Style::default().fg(ORANGE_HOT),
            ),
            Span::styled(
                format!(
                    " · {:.1} tool calls per turn · {} interrupted ({:.1}%)",
                    a.tools_per_turn(),
                    a.interrupts,
                    a.interrupts as f64 / a.turns() as f64 * 100.0,
                ),
                Style::default().fg(TEXT),
            ),
        ]));
        lines.push(note(
            "turn time as Claude Code measured it, not elapsed session time — the gaps between \
             turns are you reading and typing"
                .into(),
        ));
        lines.push(Line::from(""));

        // Tools and skills side by side: what the agent reached for, and what
        // the user did.
        lines.push(section("TOOLS  ·  SKILLS ACTUALLY RUN  (sessions · messages)"));
        let tools = tokens::Activity::top(&a.tools, 8);
        let skills = tokens::Activity::skill_ranking(a);
        let calls = a.tool_calls().max(1) as f64;
        let top_call = tools.first().map(|t| t.1).unwrap_or(1).max(1) as f64;
        for i in 0..tools.len().max(skills.len().min(8)) {
            let mut spans = match tools.get(i) {
                Some((name, n)) => vec![
                    Span::styled(format!("  {:<16}", truncate(name, 15)), Style::default().fg(TEXT)),
                    Span::styled(
                        format!("{n:>7}{:>6.1}%", *n as f64 / calls * 100.0),
                        Style::default().fg(AMBER),
                    ),
                    Span::styled(
                        format!(" {}", hbar(*n as f64 / top_call, 8)),
                        Style::default().fg(AMBER),
                    ),
                ],
                None => vec![Span::styled(" ".repeat(40), Style::default())],
            };
            if let Some((name, sess, msgs)) = skills.get(i) {
                spans.push(Span::styled(
                    format!("   {:<26}", truncate(name, 25)),
                    Style::default().fg(TEXT),
                ));
                spans.push(Span::styled(format!("{sess:>5}"), Style::default().fg(ORANGE_HOT)));
                spans.push(Span::styled(format!("{msgs:>8}"), Style::default().fg(MUTED)));
            }
            lines.push(Line::from(spans));
        }
        lines.push(note(
            "a skill here is one the harness attributed work to — evidence it RAN, not that it \
             was seeded"
                .into(),
        ));
        lines.push(Line::from(""));

        // Weekday, files and shell.
        lines.push(section("TURNS BY WEEKDAY  ·  MOST-EDITED FILES"));
        let files = tokens::Activity::top(&a.files, 7);
        let peak_day = a.turn_weekday.iter().copied().max().unwrap_or(1).max(1);
        for (i, name) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].iter().enumerate() {
            let n = a.turn_weekday[i];
            let mut spans = vec![
                Span::styled(format!("  {name}  "), Style::default().fg(TEXT)),
                Span::styled(format!("{n:>5} "), Style::default().fg(MUTED)),
                Span::styled(
                    hbar(n as f64 / peak_day as f64, 14),
                    Style::default().fg(if a.busiest_weekday() == Some(i) { ORANGE_HOT } else { AMBER }),
                ),
            ];
            if let Some((file, count)) = files.get(i) {
                spans.push(Span::styled(
                    format!("   {:<26}", truncate(file, 25)),
                    Style::default().fg(TEXT),
                ));
                spans.push(Span::styled(format!("{count:>6}"), Style::default().fg(MUTED)));
            }
            lines.push(Line::from(spans));
        }
        lines.push(Line::from(""));

        // ── Context nobody typed ────────────────────────────────────────────
        let injected = a.injected_total();
        if injected.count > 0 {
            lines.push(section("CONTEXT NOBODY TYPED  (harness injections)"));
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    format!("{} injections", injected.count),
                    Style::default().fg(TEXT),
                ),
                Span::styled(
                    format!(" · ~{} tokens · ", tokens::fmt_tokens(injected.est_tokens())),
                    Style::default().fg(AMBER),
                ),
                Span::styled(
                    format!("~{}", tokens::fmt_cost(injected.cost())),
                    Style::default().fg(ORANGE_HOT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        " ({:.1}% of all spend)",
                        if total.cost > 0.0 { injected.cost() / total.cost * 100.0 } else { 0.0 }
                    ),
                    Style::default().fg(TEXT),
                ),
            ]));
            let widest =
                a.injected_ranking().first().map(|(_, i)| i.cost()).unwrap_or(1.0).max(1e-9);
            for (kind, i) in a.injected_ranking().into_iter().take(8) {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {:<28}", truncate(&tokens::injection_label(&kind), 27)),
                        Style::default().fg(TEXT),
                    ),
                    Span::styled(format!("{:>6}", i.count), Style::default().fg(MUTED)),
                    Span::styled(
                        format!("{:>9}", tokens::fmt_tokens(i.est_tokens())),
                        Style::default().fg(AMBER),
                    ),
                    Span::styled(
                        format!("{:>9}", tokens::fmt_cost(i.cost())),
                        Style::default().fg(ORANGE_HOT),
                    ),
                    Span::styled(format!("{:>6.1}x", i.multiplier()), Style::default().fg(MUTED)),
                    Span::styled(
                        format!("  {}", hbar(i.cost() / widest, 10)),
                        Style::default().fg(ORANGE_HOT),
                    ),
                ]));
            }
            lines.push(note(
                "an injection is written once and RE-READ by every later request in its session — \
                 that is the x column, and it is where the money actually goes"
                    .into(),
            ));
            if a.attachments.keys().any(|k| k.starts_with("hook_success/")) {
                lines.push(note(
                    "a SessionStart hook is recorded TWICE (hook_success + hook_additional_context) \
                     — the '(2nd copy)' row is the same payload, not a second injection"
                        .into(),
                ));
            }
            // The one thing this section must never do is look measured.
            lines.push(note(
                "ESTIMATE — characters ÷ 4. The transcript records what was injected, never what \
                 it tokenised to, so do not add this to a total that came from a usage field"
                    .into(),
            ));
            lines.push(Line::from(""));
        }

        lines.push(section("SHELL COMMANDS  (first word)"));
        let shell = tokens::Activity::top(&a.shell, 8);
        lines.push(Line::from(Span::styled(
            format!(
                "  {}",
                shell
                    .iter()
                    .map(|(n, c)| format!("{n} {c}"))
                    .collect::<Vec<_>>()
                    .join("  ·  ")
            ),
            Style::default().fg(MUTED),
        )));
        if a.queued > 0 {
            lines.push(note(format!(
                "{} prompts queued mid-turn, {} withdrawn before they ran",
                a.queued, a.unqueued
            )));
        }
    }

    let viewport = inner.height.saturating_sub(1);
    app.tokens_scroll_max.set((lines.len() as u16).saturating_sub(viewport));

    f.render_widget(
        Paragraph::new(lines)
            .scroll((scroll, 0))
            .style(Style::default().bg(SURFACE))
            .block(Block::default().padding(Padding::new(1, 1, 0, 0))),
        inner,
    );
}

/// Clip a name to `n` characters so a long project folder can't shove a column
/// off the pane.
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// There must be exactly ONE writer of the OAuth token, and it is
    /// `main::persist_oauth_token`, because that is the only place that knows
    /// whether a vault is configured.
    ///
    /// This file had a second one: the `L` login assigned `cfg.oauth_token`
    /// directly, so a TUI login on a machine whose token had just moved to the
    /// store wrote the plaintext straight back into `config.toml` and undid the
    /// move — silently, and reported "Saved shared login token." A source-level
    /// assertion because the two paths cannot be compared any other way without
    /// mutating `AELLO_CONFIG_DIR`, which is process-global and races.
    #[test]
    fn no_second_writer_of_the_oauth_token() {
        let src = include_str!("tui.rs");
        // Assembled at runtime, never written out whole: `include_str!` pulls in
        // this test module too, so a literal needle matches its own assertion
        // and the guard fails on a clean file — a check that can only ever be
        // red is no better than one that can only ever be green.
        let needle = format!("{}{}", "oauth_token", " = Some(");
        assert!(
            !src.contains(&needle),
            "tui.rs writes the OAuth token itself — a vault-configured machine would get a \
             plaintext copy back in config.toml. Call crate::persist_oauth_token(token) instead."
        );
    }

    /// A token living in the store must not render as "no auth". It did, and
    /// the footer's remedy is `press L` — the one action that puts the
    /// plaintext back.
    #[test]
    fn a_token_in_the_vault_does_not_read_as_missing() {
        let mut app = bare_app();
        app.has_token = false;
        app.token_in_vault = true;
        let mut term =
            Terminal::new(ratatui::backend::TestBackend::new(140, 20)).expect("test terminal");
        term.draw(|f| draw(f, &app)).expect("draw");
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .concat();
        assert!(text.contains("AUTH: VAULT"), "{text}");
        assert!(!text.contains("AUTH: NONE"), "{text}");
    }

    fn press(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    /// An `App` with no config on disk, for render tests.
    fn bare_app() -> App {
        App {
            blueprints: Vec::new(),
            local: Vec::new(),
            view: Vec::new(),
            show_all: false,
            selected: 0,
            mode: Mode::Normal,
            add_agent: Agent::Claude,
            status: String::new(),
            dir: "TEST / DIR".into(),
            has_token: true,
            token_in_vault: false,
            voice_muted: false,
            help_scroll_max: std::cell::Cell::new(0),
            tokens: None,
            tokens_scroll_max: std::cell::Cell::new(0),
        }
    }

    fn sample_report() -> tokens::Report {
        use std::collections::{BTreeMap, BTreeSet};
        let u = tokens::Usage {
            input: 41_000,
            output: 2_670_000,
            cache_write_5m: 1_000_000,
            cache_write_1h: 6_650_000,
            cache_read: 744_000_000,
        };
        let mut by_model = BTreeMap::new();
        by_model.insert("claude-opus-5".to_string(), u);
        let priced = tokens::Priced {
            usage: u,
            cost: u.cost("claude-opus-5").unwrap(),
            unpriced_tokens: 0,
            by_model,
        };
        let now = tokens::now();
        let env = tokens::EnvRoll {
            blueprint: "TechnicalDirector".into(),
            projects: BTreeSet::from(["aello".to_string()]),
            priced: priced.clone(),
            sessions: vec![tokens::SessionRoll {
                id: "c95944da".into(),
                project: "aello".into(),
                first: now - 7200,
                last: now - 600,
                messages: 173,
                priced: priced.clone(),
            }],
            first: now - 90_000,
            last: now - 600,
        };
        let block = tokens::Block {
            start: now - 3600,
            last: now - 600,
            priced: priced.clone(),
            by_env: BTreeMap::from([("TechnicalDirector".to_string(), u)]),
        };
        // The stats page reads `records`, not the rollups, so a sample report
        // with none renders an empty chart — which is exactly the bug this
        // fixture would otherwise hide.
        // Two branches, two models and one record with no effort recorded, so
        // the branch table, the migration chart and the `(unrecorded)` row all
        // have something to draw. A fixture with one of each renders a
        // single-row table that looks fine and proves nothing.
        let rec = |ts: i64, project: &str, session: &str, branch: &str, model: &str, effort: Option<&str>| {
            tokens::Record {
                ts,
                model: model.into(),
                blueprint: "TechnicalDirector".into(),
                project: project.into(),
                session: session.into(),
                branch: branch.into(),
                effort: effort.map(str::to_string),
                usage: u,
            }
        };
        tokens::Report {
            records: vec![
                rec(now - 90_000, "aello", "c95944da", "main", "claude-opus-4-8", None),
                rec(now - 3_000, "aello", "c95944da", "feat-stats", "claude-opus-5", Some("high")),
                rec(now - 1_000, "revoiced", "aa11bb22", "main", "claude-opus-5", Some("medium")),
            ],
            // Likewise for activity: an all-zero default renders the whole
            // "what the sessions did" half as nothing, which would pass every
            // assertion below by simply not being there.
            activity: tokens::Activity {
                // Odd count, so the median is one of the samples rather than a
                // choice between two.
                turn_ms: vec![9_000, 93_728, 5_462_642],
                turn_hour: { let mut h = [0u64; 24]; h[15] = 3; h },
                turn_weekday: [2, 0, 0, 1, 0, 0, 0],
                tools: BTreeMap::from([
                    ("Bash".to_string(), 20),
                    ("Edit".to_string(), 12),
                    ("Read".to_string(), 8),
                ]),
                skills: BTreeMap::from([
                    ("sync".to_string(), 88),
                    ("handoff".to_string(), 11),
                ]),
                skill_sessions: BTreeMap::from([
                    ("sync".to_string(), BTreeSet::from(["s1".to_string()])),
                    ("handoff".to_string(), BTreeSet::from(["s1".to_string(), "s2".to_string()])),
                ]),
                attachments: BTreeMap::from([
                    (
                        "task_reminder".to_string(),
                        tokens::Injected {
                            count: 12,
                            chars: 1_644,
                            write_cost: 0.004,
                            read_cost: 0.021,
                        },
                    ),
                    (
                        "hook_additional_context/UserPromptSubmit".to_string(),
                        tokens::Injected {
                            count: 3,
                            chars: 5_535,
                            write_cost: 0.014,
                            read_cost: 0.062,
                        },
                    ),
                    (
                        "hook_success/SessionStart".to_string(),
                        tokens::Injected {
                            count: 1,
                            chars: 4_500,
                            write_cost: 0.011,
                            read_cost: 0.050,
                        },
                    ),
                ]),
                // Already priced above, so the raw events are spent.
                injections: Vec::new(),
                files: BTreeMap::from([("CLAUDE.md".to_string(), 6)]),
                shell: BTreeMap::from([("git".to_string(), 9)]),
                interrupts: 1,
                queued: 4,
                unqueued: 2,
            },
            envs: vec![env],
            blocks: vec![block],
            unknown_models: BTreeSet::new(),
            files_scanned: 220,
            pointer_only: 3,
            messages: 15_092,
            raw_records: 32_286,
        }
    }

    /// Renders the whole tab into an offscreen buffer. Catches the failure a
    /// layout change actually causes — a Rect wider than its parent panics
    /// inside ratatui, and that panic would only ever surface on the user's
    /// terminal. Also pins the two claims the pane must never overstate: the
    /// cost is estimated, and the window percentage is against our own peak
    /// block rather than an unknown subscription quota.
    #[test]
    fn tokens_tab_renders_and_labels_its_estimates() {
        let mut app = bare_app();
        app.tokens = Some(sample_report());
        let mut term =
            Terminal::new(ratatui::backend::TestBackend::new(140, 34)).expect("test terminal");
        term.draw(|f| draw_tokens(f, &app, 0, 0)).expect("draw");

        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .concat();

        assert!(text.contains("TOKENS // USAGE"));
        assert!(text.contains("TechnicalDirector"));
        assert!(text.contains("CURRENT 5H WINDOW"));
        assert!(text.contains("OF PEAK BLOCK"), "the denominator must be named");
        assert!(text.contains("estimated"), "cost must never read as a bill");
        assert!(text.contains("CACHE READ"));
        assert!(text.contains("SESSIONS (1)"));
    }

    /// The statistics page, rendered offscreen. Same reason as the tab above —
    /// a layout slip panics inside ratatui on the user's terminal and nowhere
    /// else — plus the two labels that stop a chart overstating: the hour
    /// histogram is UTC, and the cost is a what-if.
    #[test]
    fn token_stats_page_charts_the_projects_and_labels_its_axes() {
        let mut app = bare_app();
        app.tokens = Some(sample_report());
        let mut term =
            Terminal::new(ratatui::backend::TestBackend::new(140, 40)).expect("test terminal");
        term.draw(|f| draw_token_stats(f, &app, 0)).expect("draw");

        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .concat();

        assert!(text.contains("TOKENS // STATISTICS"));
        assert!(text.contains("TOKEN-HUNGRY PROJECTS"), "the ranking is the point");
        assert!(text.contains("tokens ÷ sessions"), "and the formula is stated");
        assert!(text.contains("aello"), "{text}");
        assert!(text.contains("revoiced"), "every project is listed, not just the cwd's");
        assert!(text.contains("WHERE THE MONEY GOES"));
        assert!(text.contains("cache read"));
        assert!(text.contains("UTC"), "the hour histogram must name its timezone");
        assert!(text.contains("what-if"), "cost must never read as a bill");
        // The two ways a total here is quietly incomplete, both stated on the
        // page rather than left for the reader to discover.
        assert!(text.contains("count as ZERO"), "pointer-only archives must be named: {text}");
        assert!(text.contains("THIS directory"), "the live half is cwd-scoped: {text}");
    }

    /// The activity half of the page: what the sessions did, as opposed to what
    /// they spent. Rendered tall enough to reach it, and asserted on the parts
    /// that carry meaning rather than on the layout.
    #[test]
    fn token_stats_page_shows_what_the_sessions_did() {
        let mut app = bare_app();
        app.tokens = Some(sample_report());
        let mut term =
            Terminal::new(ratatui::backend::TestBackend::new(140, 90)).expect("test terminal");
        term.draw(|f| draw_token_stats(f, &app, 0)).expect("draw");
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .concat();

        assert!(text.contains("WHAT THE SESSIONS DID"), "{text}");
        assert!(text.contains("3 turns"), "turn count: {text}");
        // Seconds, not `fmt_duration`'s minute resolution — the median turn is
        // 94 seconds and "1m" would be a worse answer than none.
        assert!(text.contains("1m34s"), "median turn keeps its seconds: {text}");
        assert!(text.contains("Bash"), "the tool mix is the point: {text}");
        assert!(text.contains("SKILLS ACTUALLY RUN"), "{text}");
        assert!(text.contains("handoff"), "{text}");
        assert!(text.contains("CLAUDE.md"), "most-edited files: {text}");
        // Turn time is measured, not inferred, and the page has to say so —
        // otherwise it reads as elapsed session time and the $/hour is wrong.
        assert!(text.contains("not elapsed session time"), "{text}");
    }

    /// The four splits added after the first activity pass: branch, effort,
    /// model timeline and harness-injected context. Each has one label that
    /// stops it being read as more than it is.
    #[test]
    fn token_stats_page_splits_spend_and_labels_every_estimate() {
        let mut app = bare_app();
        app.tokens = Some(sample_report());
        let mut term =
            Terminal::new(ratatui::backend::TestBackend::new(140, 110)).expect("test terminal");
        term.draw(|f| draw_token_stats(f, &app, 0)).expect("draw");
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .concat();

        assert!(text.contains("SPEND BY BRANCH"), "{text}");
        assert!(text.contains("feat-stats"), "both branches listed: {text}");
        assert!(text.contains("SPEND BY REASONING EFFORT"), "{text}");
        // An absent field gets its own row and says why, rather than being
        // folded into the commonest value.
        assert!(text.contains("(unrecorded)"), "{text}");
        assert!(text.contains("would invent the number"), "{text}");
        assert!(text.contains("MODELS OVER TIME"), "{text}");
        assert!(text.contains("claude-opus-4-8"), "the model being migrated FROM: {text}");
        // The injected-context figure is characters ÷ 4 and must never read as
        // though it came off a usage field.
        assert!(text.contains("CONTEXT NOBODY TYPED"), "{text}");
        // The hook event names the row, so the per-turn hook is identifiable
        // without matching on its wording.
        assert!(text.contains("hook: UserPromptSubmit"), "{text}");
        assert!(text.contains("ESTIMATE"), "the estimate must be labelled: {text}");
        // The two things a reader gets wrong otherwise: that an injection is a
        // one-off charge, and that a SessionStart hook recorded twice was
        // injected twice.
        assert!(text.contains("RE-READ by every later request"), "{text}");
        assert!(text.contains("2nd copy"), "the double-recorded row must say so: {text}");
    }

    /// The stats page must survive being opened before the scan finishes —
    /// `s` is reachable from a tab that itself renders a SCANNING frame.
    #[test]
    fn token_stats_page_survives_having_no_scan_yet() {
        let app = bare_app(); // tokens: None
        let mut term =
            Terminal::new(ratatui::backend::TestBackend::new(100, 20)).expect("test terminal");
        term.draw(|f| draw_token_stats(f, &app, 0)).expect("draw");
    }

    /// The pre-scan frame is a real state, not a transient: the scan takes
    /// seconds and the tab is entered before it runs.
    #[test]
    fn tokens_tab_shows_a_scanning_frame_before_any_data() {
        let app = bare_app(); // tokens: None
        let mut term =
            Terminal::new(ratatui::backend::TestBackend::new(100, 20)).expect("test terminal");
        term.draw(|f| draw_tokens(f, &app, 0, 0)).expect("draw");
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .concat();
        assert!(text.contains("SCANNING TRANSCRIPTS"));
    }

    /// A Cline env placed here is placed here. The filter asked whether
    /// `.claude-env-<name>` existed regardless of agent, so every Cline
    /// blueprint was hidden by the default view in the very directory it lives
    /// in — and the footer counted it out ("PLACED HERE · 1 OF 2").
    #[test]
    fn a_placed_cline_env_counts_as_local() {
        let dir = tempfile::tempdir().unwrap();
        let bps = vec![
            Blueprint {
                name: "bot".into(),
                model: "openai/gpt-oss-120b".into(),
                agent: Agent::Cline,
                claude_md: None,
                role: crate::models::Role::Standalone,
                mirror_root: None,
                legacy_caps: None,
            },
            Blueprint {
                name: "coder".into(),
                model: "sonnet".into(),
                agent: Agent::Claude,
                claude_md: None,
                role: crate::models::Role::Standalone,
                mirror_root: None,
                legacy_caps: None,
            },
        ];
        // Only the Cline one is placed here.
        std::fs::create_dir_all(Agent::Cline.env_dir(dir.path(), "bot")).unwrap();

        assert_eq!(local_indices_in(dir.path(), &bps), vec![0]);
    }

    #[test]
    fn ctrl_letters_never_fire_the_plain_letter_command() {
        // The dangerous three: 'u' is self-update (no prompt), 's' writes the
        // contextdb path, 'd' opens the delete modal. As a Ctrl chord they must
        // match no arm at all.
        for c in ['u', 's', 'd', 'a', 'l', 'm', 'q'] {
            let k = press(KeyCode::Char(c), KeyModifiers::CONTROL);
            assert_eq!(command_code(&k), KeyCode::Null, "Ctrl+{c} leaked through");
            assert!(is_chord(&k));
        }
        // Alt too — same reasoning, and Alt+letter is a menu convention.
        assert_eq!(
            command_code(&press(KeyCode::Char('u'), KeyModifiers::ALT)),
            KeyCode::Null
        );
    }

    #[test]
    fn ctrl_c_quits_from_every_mode() {
        assert!(quits_everywhere(&press(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        // Not the bare letter — 'c' alone opens the contextdb picker.
        assert!(!quits_everywhere(&press(KeyCode::Char('c'), KeyModifiers::NONE)));
        assert!(!quits_everywhere(&press(KeyCode::Char('d'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn command_keys_are_case_insensitive() {
        // The delete modal renders "[Y] CONFIRM · [N] CANCEL"; a user following
        // it literally presses Shift+Y, and every handler binds lowercase.
        assert_eq!(
            command_code(&press(KeyCode::Char('Y'), KeyModifiers::SHIFT)),
            KeyCode::Char('y')
        );
        // Caps Lock reports no modifier at all.
        assert_eq!(command_code(&press(KeyCode::Char('Q'), KeyModifiers::NONE)), KeyCode::Char('q'));
        // Non-letter keys pass through untouched.
        assert_eq!(command_code(&press(KeyCode::Enter, KeyModifiers::NONE)), KeyCode::Enter);
        assert_eq!(command_code(&press(KeyCode::Esc, KeyModifiers::NONE)), KeyCode::Esc);
        assert_eq!(command_code(&press(KeyCode::Down, KeyModifiers::NONE)), KeyCode::Down);
    }

    #[test]
    fn shift_still_types_capitals_into_a_name() {
        // Text entry keeps `key.code`, so a blueprint name can be capitalised —
        // only Ctrl/Alt are filtered out there.
        let k = press(KeyCode::Char('T'), KeyModifiers::SHIFT);
        assert!(!is_chord(&k));
        assert!(is_chord(&press(KeyCode::Char('t'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn personas_match_builtins() {
        // Same three values, compared as sets: the picker leads with "none"
        // because index 0 is what an unrecognised value falls back to, while
        // BUILTINS lists the coding default first. Order differing is fine;
        // the sets differing means a value exists that cannot be picked.
        let mut picker: Vec<&str> = PERSONAS.iter().map(|(id, _)| *id).collect();
        let mut builtins: Vec<&str> = crate::templates::BUILTINS.to_vec();
        picker.sort_unstable();
        builtins.sort_unstable();
        assert_eq!(picker, builtins);
    }

    #[test]
    fn edit_preserves_untouched_values() {
        // Editing a CLI-configured blueprint whose model is a full id / persona
        // is a custom path: an untouched picker must NOT downgrade them.
        let full_id = "claude-opus-4-8".to_string();
        assert_eq!(
            resolved_edit(true, false, &full_id, MODELS[0].0.to_string()),
            "claude-opus-4-8",
            "untouched model picker must keep the full id"
        );
        let custom = Some("/path/persona.md".to_string());
        assert_eq!(
            resolved_edit(true, false, &custom, Some(PERSONAS[0].0.to_string())),
            Some("/path/persona.md".to_string()),
            "untouched persona picker must keep the custom path"
        );
        // A touched picker uses the new choice.
        assert_eq!(resolved_edit(true, true, &full_id, "sonnet".to_string()), "sonnet");
        // On add, always use the picked value regardless of orig.
        assert_eq!(resolved_edit(false, false, &full_id, "haiku".to_string()), "haiku");
    }

    /// The picker indexes straight into `Role::ALL`, so a reorder there must not
    /// silently shift what `Enter` saves in edit mode.
    #[test]
    fn role_picker_round_trips_every_role() {
        for r in Role::ALL {
            assert_eq!(Role::ALL[role_index(*r)], *r);
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
        assert_eq!(persona_index(Some("custom")), 2);
        // Unknown values fall back to index 0 (opus / "none").
        assert_eq!(model_index("claude-opus-4-8"), 0);
        assert_eq!(persona_index(None), 0);
        assert_eq!(persona_index(Some("/custom/path.md")), 0);
    }
}

