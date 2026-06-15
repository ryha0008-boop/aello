//! Minimal full-screen TUI — `aello` with no args lands here.
//!
//! Browse blueprints, add, delete, self-update, quit. Run/edit/etc. fill in as
//! later phases land. Built on ratatui + crossterm (cross-platform).

use anyhow::Result;
use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};

use crate::config;
use crate::models::Blueprint;

type Term = Terminal<CrosstermBackend<Stdout>>;

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
                        app.status = "No blueprints to delete.".into();
                    } else {
                        app.mode = Mode::ConfirmDelete;
                    }
                }
                _ => {}
            },
            Mode::Adding { name, buf } => match key.code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "Cancelled.".into();
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
                                app.status = format!("'{value}' already exists.");
                            }
                            Ok(()) => {
                                app.mode = Mode::Adding { name: Some(value), buf: String::new() };
                            }
                            Err(e) => app.status = e.to_string(),
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
                                app.status = format!("Added '{n}'.");
                                app.mode = Mode::Normal;
                                app.reload()?;
                            }
                            Err(e) => app.status = e.to_string(),
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
                    app.status = format!("Removed '{target}'.");
                    app.mode = Mode::Normal;
                    app.reload()?;
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.status = "Cancelled.".into();
                }
                _ => {}
            },
        }
    }
}

fn draw(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(2)])
        .split(f.area());

    // Title.
    let title = Paragraph::new(format!(" aello v{}", env!("CARGO_PKG_VERSION")))
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(title, chunks[0]);

    // Blueprint list.
    let items: Vec<ListItem> = if app.blueprints.is_empty() {
        vec![ListItem::new("  (no blueprints — press 'a' to add)")]
    } else {
        app.blueprints
            .iter()
            .map(|b| {
                let md = b.claude_md.as_deref().unwrap_or("-");
                ListItem::new(format!("{:<16} {:<12} {}", b.name, b.model, md))
            })
            .collect()
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" blueprints "))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");
    let mut state = ListState::default();
    if !app.blueprints.is_empty() {
        state.select(Some(app.selected));
    }
    f.render_stateful_widget(list, chunks[1], &mut state);

    // Footer: help line driven by mode, plus the status line.
    let help = match &app.mode {
        Mode::Normal => "↑/↓ move · a add · d delete · u update · q quit · (run: soon)".to_string(),
        Mode::Adding { name: None, buf } => format!("name: {buf}_   (Enter to confirm · Esc cancel)"),
        Mode::Adding { name: Some(n), buf } => {
            format!("name={n}  model: {buf}_   (Enter to confirm · Esc cancel)")
        }
        Mode::ConfirmDelete => {
            let n = &app.blueprints[app.selected].name;
            format!("Delete '{n}'?  y / n")
        }
    };
    let footer = Paragraph::new(format!("{help}\n{}", app.status))
        .style(Style::default().fg(Color::Gray));
    f.render_widget(footer, chunks[2]);
}
