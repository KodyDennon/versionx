//! Interactive ratatui dashboard (launched by `versionx tui`).
//!
//! v0.3 preview: a single-screen component dashboard.
//!
//! * Left pane — scrollable component list (id · kind · version · dirty flag).
//! * Right pane — details for the selected component (root, inputs,
//!   current/last hash, dependencies, cascade).
//! * Footer — keybind hints.
//!
//! Controls: `↑/↓` or `j/k` move selection, `g/G` jump top/bottom,
//! `r` refresh, `q` quit.

#![deny(unsafe_code)]

use std::io;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

fn main() -> Result<()> {
    let cwd = std::env::current_dir().context("resolving cwd")?;
    let root = Utf8PathBuf::from_path_buf(cwd)
        .map_err(|p| anyhow::anyhow!("cwd must be UTF-8: {}", p.to_string_lossy()))?;

    let mut app = App::load(root)?;
    let mut terminal = init_terminal()?;
    let res = run(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    res
}

// --------- App state ----------------------------------------------------

struct App {
    root: Utf8PathBuf,
    snapshot: Snapshot,
    list_state: ListState,
    last_refresh: Instant,
    status: String,
}

#[derive(Default)]
struct Snapshot {
    components: Vec<ComponentEntry>,
    topo: Vec<String>,
    /// Cascade map: component id -> ids that transitively depend on it.
    cascade: std::collections::BTreeMap<String, Vec<String>>,
}

struct ComponentEntry {
    id: String,
    kind: String,
    version: Option<String>,
    root: Utf8PathBuf,
    inputs: Vec<String>,
    deps: Vec<String>,
    dirty: bool,
    current_hash: String,
    last_hash: Option<String>,
}

impl App {
    fn load(root: Utf8PathBuf) -> Result<Self> {
        let snapshot = Self::take_snapshot(&root)?;
        let mut state = ListState::default();
        if !snapshot.components.is_empty() {
            state.select(Some(0));
        }
        let n = snapshot.components.len();
        Ok(Self {
            root,
            snapshot,
            list_state: state,
            last_refresh: Instant::now(),
            status: format!("loaded {n} components"),
        })
    }

    fn refresh(&mut self) -> Result<()> {
        self.snapshot = Self::take_snapshot(&self.root)?;
        if self.snapshot.components.is_empty() {
            self.list_state.select(None);
        } else if self.list_state.selected().is_none() {
            self.list_state.select(Some(0));
        }
        self.last_refresh = Instant::now();
        self.status = format!("refreshed ({} components)", self.snapshot.components.len());
        Ok(())
    }

    fn take_snapshot(root: &camino::Utf8Path) -> Result<Snapshot> {
        use versionx_core::commands::workspace as ws;
        use versionx_core::{EventBus, commands::CoreContext};

        // TUI snapshots don't mutate anything — use a throwaway event bus.
        let bus = EventBus::new();
        let ctx = CoreContext::detect(bus.sender()).context("detecting VERSIONX_HOME")?;

        let list = ws::list(&ctx, &ws::ListOptions { root: root.to_path_buf() })
            .context("workspace list")?;
        let status = ws::status(&ctx, &ws::StatusOptions { root: root.to_path_buf() })
            .context("workspace status")?;
        let graph = ws::graph(&ctx, &ws::GraphOptions { root: root.to_path_buf() })
            .context("workspace graph")?;

        let status_map: std::collections::HashMap<_, _> =
            status.components.iter().map(|c| (c.id.clone(), c)).collect();

        let mut components = Vec::with_capacity(list.components.len());
        for entry in list.components {
            let s = status_map.get(&entry.id);
            components.push(ComponentEntry {
                id: entry.id.clone(),
                kind: entry.kind,
                version: entry.version,
                root: entry.root,
                inputs: Vec::new(), // not returned by `list` today; 0.3 polish.
                deps: entry.depends_on,
                dirty: s.is_some_and(|s| s.dirty),
                current_hash: s.map_or_else(String::new, |s| s.current_hash.clone()),
                last_hash: s.and_then(|s| s.last_hash.clone()),
            });
        }

        let mut cascade: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for c in &status.components {
            cascade.insert(c.id.clone(), c.cascade.clone());
        }

        Ok(Snapshot { components, topo: graph.topo_order, cascade })
    }

    fn selected(&self) -> Option<&ComponentEntry> {
        self.list_state.selected().and_then(|i| self.snapshot.components.get(i))
    }

    fn move_selection(&mut self, delta: i32) {
        let n = self.snapshot.components.len();
        if n == 0 {
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0);
        let last = n - 1;
        let next = if delta >= 0 {
            cur.saturating_add(delta as usize).min(last)
        } else {
            cur.saturating_sub((-delta) as usize)
        };
        self.list_state.select(Some(next));
    }

    fn jump_start(&mut self) {
        if !self.snapshot.components.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    fn jump_end(&mut self) {
        let n = self.snapshot.components.len();
        if n > 0 {
            self.list_state.select(Some(n - 1));
        }
    }
}

// --------- Render + event loop ------------------------------------------

fn run<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| draw(f, app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
                KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
                KeyCode::Char('g') | KeyCode::Home => app.jump_start(),
                KeyCode::Char('G') | KeyCode::End => app.jump_end(),
                KeyCode::Char('r') => {
                    if let Err(err) = app.refresh() {
                        app.status = format!("refresh failed: {err}");
                    }
                }
                KeyCode::PageDown => app.move_selection(10),
                KeyCode::PageUp => app.move_selection(-10),
                _ => {}
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
}

fn draw(frame: &mut ratatui::Frame, app: &App) {
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5), Constraint::Length(3)])
        .split(frame.area());

    draw_header(frame, vchunks[0], app);

    let hchunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(vchunks[1]);

    draw_list(frame, hchunks[0], app);
    draw_details(frame, hchunks[1], app);
    draw_footer(frame, vchunks[2], app);
}

fn draw_header(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let dirty = app.snapshot.components.iter().filter(|c| c.dirty).count();
    let total = app.snapshot.components.len();
    let status_color = if dirty == 0 { Color::Green } else { Color::Yellow };
    let lines = vec![Line::from(vec![
        Span::styled("versionx ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(env!("CARGO_PKG_VERSION")),
        Span::raw("  ·  "),
        Span::raw(app.root.to_string()),
        Span::raw("  ·  "),
        Span::styled(
            format!("{dirty}/{total} dirty"),
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        ),
    ])];
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" dashboard ")),
        area,
    );
}

fn draw_list(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app
        .snapshot
        .components
        .iter()
        .map(|c| {
            let version = c.version.as_deref().unwrap_or("—");
            let marker = if c.dirty { "●" } else { "○" };
            let color = if c.dirty { Color::Yellow } else { Color::DarkGray };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {marker} "), Style::default().fg(color)),
                Span::styled(
                    format!("{:<28}", c.id),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {:<8}", c.kind), Style::default().fg(Color::Cyan)),
                Span::raw(format!(" v{version}")),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" components "))
        .highlight_style(Style::default().bg(Color::Indexed(236)).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut app.list_state.clone());
}

fn draw_details(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let Some(c) = app.selected() else {
        frame.render_widget(
            Paragraph::new("no component selected")
                .block(Block::default().borders(Borders::ALL).title(" details ")),
            area,
        );
        return;
    };

    let version = c.version.as_deref().unwrap_or("(unset)");
    let last_hash = c.last_hash.as_deref().unwrap_or("(never released)");
    let cascade = app.snapshot.cascade.get(&c.id);
    let mut lines = vec![
        Line::from(vec![
            Span::styled("id:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(c.id.clone(), Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("kind:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(c.kind.clone(), Style::default().fg(Color::Cyan)),
            Span::raw("   "),
            Span::styled("version: ", Style::default().fg(Color::DarkGray)),
            Span::raw(version),
        ]),
        Line::from(vec![
            Span::styled("root:    ", Style::default().fg(Color::DarkGray)),
            Span::raw(c.root.to_string()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("current: ", Style::default().fg(Color::DarkGray)),
            Span::raw(c.current_hash.clone()),
        ]),
        Line::from(vec![
            Span::styled("last:    ", Style::default().fg(Color::DarkGray)),
            Span::raw(last_hash.to_string()),
        ]),
        Line::from(vec![
            Span::styled("state:   ", Style::default().fg(Color::DarkGray)),
            if c.dirty {
                Span::styled(
                    "dirty",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("clean", Style::default().fg(Color::Green))
            },
        ]),
        Line::from(""),
    ];

    if c.deps.is_empty() {
        lines.push(Line::from(Span::styled("deps:    —", Style::default().fg(Color::DarkGray))));
    } else {
        lines.push(Line::from(Span::styled("deps:", Style::default().fg(Color::DarkGray))));
        for d in &c.deps {
            lines.push(Line::from(format!("  · {d}")));
        }
    }
    lines.push(Line::from(""));
    if let Some(cs) = cascade
        && !cs.is_empty()
    {
        lines.push(Line::from(Span::styled(
            format!("cascade ({}):", cs.len()),
            Style::default().fg(Color::DarkGray),
        )));
        for id in cs {
            lines.push(Line::from(format!("  → {id}")));
        }
    } else {
        lines.push(Line::from(Span::styled("cascade: —", Style::default().fg(Color::DarkGray))));
    }
    lines.push(Line::from(""));
    if !c.inputs.is_empty() {
        lines.push(Line::from(Span::styled("inputs:", Style::default().fg(Color::DarkGray))));
        for g in &c.inputs {
            lines.push(Line::from(format!("  · {g}")));
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" details ")),
        area,
    );
}

fn draw_footer(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let topo_len = app.snapshot.topo.len();
    let footer = Line::from(vec![
        Span::styled(" j/k ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("move  "),
        Span::styled(" g/G ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("jump  "),
        Span::styled(" r ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("refresh  "),
        Span::styled(" q ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("quit    "),
        Span::styled(format!("·  topo: {topo_len} nodes"), Style::default().fg(Color::DarkGray)),
        Span::raw("  ·  "),
        Span::styled(app.status.clone(), Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(footer).block(Block::default().borders(Borders::ALL)), area);
}

// --------- Terminal plumbing --------------------------------------------

fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal<B: Backend + io::Write>(terminal: &mut Terminal<B>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}
