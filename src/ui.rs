use crate::db::{ArtifactView, ErrorView, LogView, PackageView};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Bar, BarChart, BarGroup, Block, BorderType, Borders, Cell, Clear, Gauge, Paragraph, Row,
        Scrollbar, ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableState, Tabs, Wrap,
    },
    Frame, Terminal,
};
use std::io;
use std::time::{Duration, Instant};

// ─── Palette ────────────────────────────────────────────────────────────────

const C_BG: Color = Color::Rgb(13, 13, 20);
const C_SURFACE: Color = Color::Rgb(22, 22, 32);
const C_SURFACE2: Color = Color::Rgb(30, 30, 44);
const C_BORDER: Color = Color::Rgb(50, 50, 70);
const C_BORDER_ACTIVE: Color = Color::Rgb(120, 100, 220);
const C_TEXT: Color = Color::Rgb(220, 220, 230);
const C_TEXT_DIM: Color = Color::Rgb(110, 110, 140);
const C_TEXT_FAINT: Color = Color::Rgb(60, 60, 80);
const C_ACCENT: Color = Color::Rgb(140, 110, 255);
const C_ACCENT2: Color = Color::Rgb(80, 200, 160);
const C_GREEN: Color = Color::Rgb(80, 200, 130);
const C_RED: Color = Color::Rgb(230, 80, 80);
const C_AMBER: Color = Color::Rgb(240, 170, 50);
const C_BLUE: Color = Color::Rgb(80, 160, 240);
const C_SEL_BG: Color = Color::Rgb(50, 40, 90);

// ─── Onglets ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Logs,
    Artifacts,
    Packages,
    Errors,
    Analytics,
}

impl Tab {
    fn all() -> &'static [Tab] {
        &[Tab::Logs, Tab::Artifacts, Tab::Packages, Tab::Errors, Tab::Analytics]
    }
    fn index(self) -> usize {
        Tab::all().iter().position(|&t| t == self).unwrap_or(0)
    }
}

// ─── Overlay d'extraction ────────────────────────────────────────────────────

#[derive(Clone)]
pub enum OverlayState {
    Hidden,
    Running { message: String, spinner_tick: u8 },
    Done { message: String },
    Error { message: String },
    ArtifactDetail { name: String, status: String, error: Option<String> },
    LoadMore,
}

// ─── App State ───────────────────────────────────────────────────────────────

pub struct App {
    pub active_tab: Tab,
    pub table_state: TableState,

    pub logs: Vec<LogView>,
    pub artifacts: Vec<ArtifactView>,
    pub packages: Vec<PackageView>,
    pub errors: Vec<ErrorView>,

    // Graphiques
    pub error_sparkline: Vec<u64>,
    pub error_barchart: Vec<(String, u64)>,

    // Analytics
    pub activity_sparkline: Vec<u64>,
    pub top_errors_barchart: Vec<(String, u64)>,
    pub status_counts: Vec<(String, u64)>,

    // Filtres et Recherche
    pub search_query: String,
    pub search_active: bool,
    pub filter_failed: bool,
    pub filtered_logs: Vec<LogView>,
    pub filtered_artifacts: Vec<ArtifactView>,
    pub filtered_packages: Vec<PackageView>,
    pub filtered_errors: Vec<ErrorView>,

    pub stats: Stats,
    pub logs_limit: u32,
    pub should_quit: bool,
    pub overlay: OverlayState,
    pub last_tick: Instant,
    pub last_refresh: Instant,
}

pub struct Stats {
    pub total_logs: i64,
    pub failed_logs: i64,
    pub total_packages: i64,
    pub total_artifacts: i64,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            total_logs: 0,
            failed_logs: 0,
            total_packages: 0,
            total_artifacts: 0,
        }
    }
}

impl App {
    pub fn new() -> Self {
        let mut ts = TableState::default();
        ts.select(Some(0));
        Self {
            active_tab: Tab::Logs,
            table_state: ts,
            logs: vec![],
            artifacts: vec![],
            packages: vec![],
            errors: vec![],
            error_sparkline: vec![],
            error_barchart: vec![],
            activity_sparkline: vec![],
            top_errors_barchart: vec![],
            status_counts: vec![],
            search_query: String::new(),
            search_active: false,
            filter_failed: false,
            filtered_logs: vec![],
            filtered_artifacts: vec![],
            filtered_packages: vec![],
            filtered_errors: vec![],
            stats: Stats::default(),
            logs_limit: 200,
            should_quit: false,
            overlay: OverlayState::Hidden,
            last_tick: Instant::now(),
            last_refresh: Instant::now(),
        }
    }

    pub fn apply_filters(&mut self) {
        let q = self.search_query.to_lowercase();

        self.filtered_logs = self.logs.iter().filter(|l| {
            let failed_ok = !self.filter_failed || l.status.as_deref() == Some("FAILED");
            let search_ok = q.is_empty()
                || l.status.as_deref().unwrap_or("").to_lowercase().contains(&q)
                || l.error_message.as_deref().unwrap_or("").to_lowercase().contains(&q)
                || l.message_guid.as_deref().unwrap_or("").to_lowercase().contains(&q);
            failed_ok && search_ok
        }).cloned().collect();

        self.filtered_artifacts = self.artifacts.iter().filter(|a| {
            q.is_empty()
                || a.name.as_deref().unwrap_or("").to_lowercase().contains(&q)
                || a.status.as_deref().unwrap_or("").to_lowercase().contains(&q)
        }).cloned().collect();

        self.filtered_packages = self.packages.iter().filter(|p| {
            q.is_empty()
                || p.id.as_deref().unwrap_or("").to_lowercase().contains(&q)
                || p.name.as_deref().unwrap_or("").to_lowercase().contains(&q)
                || p.vendor.as_deref().unwrap_or("").to_lowercase().contains(&q)
        }).cloned().collect();

        self.filtered_errors = self.errors.iter().filter(|e| {
            q.is_empty()
                || e.artifact_id.to_lowercase().contains(&q)
                || e.error_message.as_deref().unwrap_or("").to_lowercase().contains(&q)
        }).cloned().collect();

        let len = self.current_len();
        if let Some(i) = self.table_state.selected() {
            if i >= len && len > 0 {
                self.table_state.select(Some(len - 1));
            } else if len == 0 {
                self.table_state.select(None);
            }
        }
    }

    pub fn current_len(&self) -> usize {
        match self.active_tab {
            Tab::Logs => self.filtered_logs.len(),
            Tab::Artifacts => self.filtered_artifacts.len(),
            Tab::Packages => self.filtered_packages.len(),
            Tab::Errors => self.filtered_errors.len(),
            Tab::Analytics => 0,
        }
    }

    pub fn next_row(&mut self) {
        let len = self.current_len();
        if len == 0 {
            return;
        }
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some((i + 1).min(len - 1)));
    }

    pub fn prev_row(&mut self) {
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some(i.saturating_sub(1)));
    }

    pub fn next_tab(&mut self) {
        let tabs = Tab::all();
        let i = (self.active_tab.index() + 1) % tabs.len();
        self.active_tab = tabs[i];
        self.table_state.select(Some(0));
    }

    pub fn prev_tab(&mut self) {
        let tabs = Tab::all();
        let i = self.active_tab.index();
        let prev = if i == 0 { tabs.len() - 1 } else { i - 1 };
        self.active_tab = tabs[prev];
        self.table_state.select(Some(0));
    }

    pub fn tick(&mut self) {
        if let OverlayState::Running {
            ref mut spinner_tick,
            ..
        } = self.overlay
        {
            *spinner_tick = spinner_tick.wrapping_add(1);
        }
    }
}

// ─── Entrée TUI ──────────────────────────────────────────────────────────────

pub fn run_tui(app: &mut App) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tick_rate = Duration::from_millis(120);
    let result = run_loop(&mut terminal, app, tick_rate);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    tick_rate: Duration,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| draw(f, app))?;

        let timeout = tick_rate
            .checked_sub(app.last_tick.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    app.should_quit = true;
                }

                match &app.overlay {
                    OverlayState::Done { .. } | OverlayState::Error { .. } | OverlayState::ArtifactDetail { .. } => {
                        if matches!(key.code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q')) {
                            app.overlay = OverlayState::Hidden;
                        }
                        continue;
                    }
                    OverlayState::Running { .. } | OverlayState::LoadMore => return Ok(()),
                    OverlayState::Hidden => {}
                }

                // GESTION DU MODE RECHERCHE
                if app.search_active {
                    match key.code {
                        KeyCode::Esc => {
                            app.search_active = false;
                            app.search_query.clear();
                            app.apply_filters();
                        }
                        KeyCode::Enter => {
                            app.search_active = false;
                        }
                        KeyCode::Backspace => {
                            app.search_query.pop();
                            app.apply_filters();
                        }
                        KeyCode::Char(c) => {
                            app.search_query.push(c);
                            app.apply_filters();
                        }
                        _ => {}
                    }
                    continue;
                }

                // NAVIGATION NORMALE
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                    KeyCode::Char('f') if app.active_tab == Tab::Logs => {
                        app.filter_failed = !app.filter_failed;
                        app.apply_filters();
                    }
                    KeyCode::Tab => app.next_tab(),
                    KeyCode::BackTab => app.prev_tab(),
                    KeyCode::Right | KeyCode::Char('l') => app.next_tab(),
                    KeyCode::Left | KeyCode::Char('h') => app.prev_tab(),
                    KeyCode::Down | KeyCode::Char('j') => app.next_row(),
                    KeyCode::Up | KeyCode::Char('k') => app.prev_row(),
                    KeyCode::Char('r') => {
                        app.overlay = OverlayState::Running {
                            message: "Extraction en cours...".to_string(),
                            spinner_tick: 0,
                        };
                        return Ok(());
                    }
                    KeyCode::Enter if app.active_tab == Tab::Artifacts => {
                        if let Some(i) = app.table_state.selected() {
                            if let Some(art) = app.filtered_artifacts.get(i) {
                                let error = app.errors.iter()
                                    .find(|e| Some(e.artifact_id.as_str()) == art.name.as_deref())
                                    .and_then(|e| e.error_message.clone());
                                app.overlay = OverlayState::ArtifactDetail {
                                    name: art.name.clone().unwrap_or_default(),
                                    status: art.status.clone().unwrap_or_default(),
                                    error,
                                };
                            }
                        }
                    }
                    KeyCode::Char('+') => {
                        app.logs_limit += 500;
                        app.overlay = OverlayState::LoadMore;
                        return Ok(());
                    }
                    KeyCode::Char(c) if app.active_tab != Tab::Analytics => {
                        app.search_active = true;
                        app.search_query.push(c);
                        app.apply_filters();
                    }
                    _ => {}
                }
            }
        }

        if app.last_tick.elapsed() >= tick_rate {
            app.tick();
            app.last_tick = Instant::now();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

// ─── Rendu principal ─────────────────────────────────────────────────────────

fn draw(f: &mut Frame, app: &mut App) {
    let size = f.size();
    f.render_widget(Block::default().style(Style::default().bg(C_BG)), size);

    // Layout modifié : la zone du haut passe de Length(5) à Length(10) pour les charts
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(10),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(size);

    draw_header(f, app, chunks[0]);
    draw_stats_and_charts(f, app, chunks[1]);
    draw_body(f, app, chunks[2]);
    draw_footer(f, app, chunks[3]);

    if !matches!(app.overlay, OverlayState::Hidden) {
        draw_overlay(f, app, size);
    }
}

// ─── Header / Tabs ───────────────────────────────────────────────────────────

fn tab_label(tab: Tab, app: &App) -> Line<'static> {
    let (name, count, is_alert) = match tab {
        Tab::Logs => ("Logs", app.filtered_logs.len(), false),
        Tab::Artifacts => ("Artifacts", app.filtered_artifacts.len(), false),
        Tab::Packages => ("Packages", app.filtered_packages.len(), false),
        Tab::Errors => ("Erreurs", app.filtered_errors.len(), !app.filtered_errors.is_empty()),
        Tab::Analytics => ("Analytics", 0, false),
    };
    let badge_color = if is_alert { C_RED } else { C_TEXT_FAINT };
    Line::from(vec![
        Span::raw(format!("  {} ", name)),
        Span::styled(format!("[{}]  ", count), Style::default().fg(badge_color)),
    ])
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(26), Constraint::Min(0)])
        .split(area);

    let age = app.last_refresh.elapsed().as_secs();
    let freshness = if age < 60 {
        format!("{}s", age)
    } else {
        format!("{}m{}s", age / 60, age % 60)
    };
    let freshness_color = if age > 300 { C_RED } else { C_TEXT_FAINT };

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "SAP",
            Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " BTP",
            Style::default().fg(C_ACCENT2).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · Monitor  ", Style::default().fg(C_TEXT_DIM)),
        Span::styled(
            format!("↻ {}  ", freshness),
            Style::default().fg(freshness_color),
        ),
        Span::styled(
            format!("[{}] logs  +500", app.logs_limit),
            Style::default().fg(C_TEXT_FAINT),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(C_BORDER))
            .style(Style::default().bg(C_SURFACE)),
    )
    .alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    let tab_labels: Vec<Line> = Tab::all()
        .iter()
        .map(|&t| {
            let line = tab_label(t, app);

            if t == app.active_tab {
                line.patch_style(Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD))
            } else {
                line.patch_style(Style::default().fg(C_TEXT_DIM))
            }
        })
        .collect();

    let tabs = Tabs::new(tab_labels)
        .select(app.active_tab.index())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(C_BORDER))
                .style(Style::default().bg(C_SURFACE)),
        )
        .highlight_style(
            Style::default()
                .fg(C_ACCENT)
                .bg(C_SURFACE2)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled(" │ ", Style::default().fg(C_BORDER)));

    f.render_widget(tabs, chunks[1]);
}

// ─── Stats & Charts ──────────────────────────────────────────────────────────

fn draw_stats_and_charts(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // GAUCHE : 4 stat cards (2x2)
    let stat_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(cols[0]);
    let top_stats = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(stat_rows[0]);
    let bot_stats = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(stat_rows[1]);

    let cards = [
        (
            "TOTAL LOGS",
            app.stats.total_logs.to_string(),
            C_BLUE,
            "▲",
            top_stats[0],
        ),
        (
            "ERREURS",
            app.stats.failed_logs.to_string(),
            if app.stats.failed_logs > 0 {
                C_RED
            } else {
                C_GREEN
            },
            "!",
            top_stats[1],
        ),
        (
            "PACKAGES",
            app.stats.total_packages.to_string(),
            C_ACCENT,
            "◈",
            bot_stats[0],
        ),
        (
            "ARTIFACTS",
            app.stats.total_artifacts.to_string(),
            C_ACCENT2,
            "◉",
            bot_stats[1],
        ),
    ];

    for (label, value, color, icon, rect) in cards {
        let content = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(format!(" {} ", icon), Style::default().fg(color)),
                Span::styled(
                    value,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                format!("  {}", label),
                Style::default().fg(C_TEXT_FAINT),
            )),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(C_BORDER))
            .style(Style::default().bg(C_SURFACE));
        f.render_widget(Paragraph::new(content).block(block), rect);
    }

    // DROITE : Charts
    let chart_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(cols[1]);

    let spark = Sparkline::default()
        .block(
            Block::default()
                .title(" Erreurs / heure (24h) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(C_BORDER))
                .border_type(BorderType::Rounded),
        )
        .data(&app.error_sparkline)
        .style(Style::default().fg(C_RED));
    f.render_widget(spark, chart_rows[0]);

    let bars: Vec<Bar> = app
        .error_barchart
        .iter()
        .map(|(label, val)| Bar::default().value(*val).label(label.as_str().into()))
        .collect();
    let bc = BarChart::default()
        .block(
            Block::default()
                .title(" Erreurs / jour (7j) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(C_BORDER))
                .border_type(BorderType::Rounded),
        )
        .data(BarGroup::default().bars(&bars))
        .bar_width(5)
        .bar_gap(1)
        .bar_style(Style::default().fg(C_AMBER))
        .value_style(Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD));
    f.render_widget(bc, chart_rows[1]);
}

// ─── Body (tableaux) ─────────────────────────────────────────────────────────

fn draw_body(f: &mut Frame, app: &mut App, area: Rect) {
    match app.active_tab {
        Tab::Logs => draw_logs_master_detail(f, app, area),
        Tab::Artifacts => draw_artifacts_table(f, app, area),
        Tab::Packages => draw_packages_table(f, app, area),
        Tab::Errors => draw_errors_table(f, app, area),
        Tab::Analytics => draw_analytics(f, app, area),
    }
}

fn table_block(title: &str, count: usize) -> Block {
    Block::default()
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", title),
                Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("({}) ", count), Style::default().fg(C_TEXT_DIM)),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(C_BORDER_ACTIVE))
        .style(Style::default().bg(C_SURFACE))
}

fn header_row(cells: &[&str]) -> Row<'static> {
    Row::new(
        cells
            .iter()
            .map(|c| {
                Cell::from(c.to_string()).style(
                    Style::default()
                        .fg(C_TEXT_DIM)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                )
            })
            .collect::<Vec<_>>(),
    )
    .height(1)
    .style(Style::default().bg(C_SURFACE2))
}

fn status_style(status: &str) -> Style {
    match status {
        "COMPLETED" => Style::default().fg(C_GREEN).add_modifier(Modifier::BOLD),
        "FAILED" => Style::default().fg(C_RED).add_modifier(Modifier::BOLD),
        "ERROR" => Style::default().fg(C_RED).add_modifier(Modifier::BOLD),
        "STARTING" | "PROCESSING" => Style::default().fg(C_AMBER),
        "STARTED" => Style::default().fg(C_BLUE),
        _ => Style::default().fg(C_TEXT_DIM),
    }
}

fn status_icon(status: &str) -> &'static str {
    match status {
        "COMPLETED" | "STARTED" => "●",
        "FAILED" | "ERROR" => "●",
        "STARTING" | "PROCESSING" => "◌",
        _ => "○",
    }
}

// Vue Split Screen pour les Logs
fn draw_logs_master_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    draw_logs_table(f, app, chunks[0]);

    let detail_text = app
        .table_state
        .selected()
        .and_then(|i| app.filtered_logs.get(i))
        .map(|log| {
            let date = log
                .parsed_date
                .map(|d| d.format("%d/%m/%Y %H:%M:%S").to_string())
                .unwrap_or_default();
            let guid = log.message_guid.as_deref().unwrap_or("—");
            let status = log.status.as_deref().unwrap_or("—");
            let err = log
                .error_message
                .as_deref()
                .unwrap_or("Aucune information d'erreur.");
            format!(
                "GUID    : {}\nDate    : {}\nStatut  : {}\n\nErreur  :\n{}",
                guid, date, status, err
            )
        })
        .unwrap_or_else(|| "Sélectionnez une ligne pour voir le détail technique.".to_string());

    let status = app
        .table_state
        .selected()
        .and_then(|i| app.filtered_logs.get(i))
        .and_then(|l| l.status.as_deref())
        .unwrap_or("");

    let border_color = match status {
        "FAILED" => C_RED,
        "COMPLETED" => C_GREEN,
        _ => C_BORDER,
    };

    let detail = Paragraph::new(detail_text)
        .block(
            Block::default()
                .title(Span::styled(
                    " Vue Détaillée ",
                    Style::default()
                        .fg(border_color)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color))
                .style(Style::default().bg(C_SURFACE)),
        )
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(C_TEXT));

    f.render_widget(detail, chunks[1]);
}

fn draw_logs_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header = header_row(&["  Statut", "ID Message", "Date", "Heure", "Aperçu erreur"]);

    let rows: Vec<Row> = app
        .filtered_logs
        .iter()
        .map(|log| {
            let status = log.status.as_deref().unwrap_or("—");
            let (date_str, time_str) = log
                .parsed_date
                .map(|d| {
                    (
                        d.format("%d/%m/%Y").to_string(),
                        d.format("%H:%M:%S").to_string(),
                    )
                })
                .unwrap_or(("—".into(), "—".into()));

            let guid = log.message_guid.as_deref().unwrap_or("—");
            let err = log
                .error_message
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(60)
                .collect::<String>();

            let icon = status_icon(status);
            let sty = status_style(status);

            Row::new(vec![
                Cell::from(format!("  {} {}", icon, status)).style(sty),
                Cell::from(guid).style(Style::default().fg(C_ACCENT)),
                Cell::from(date_str).style(Style::default().fg(C_TEXT_DIM)),
                Cell::from(time_str).style(Style::default().fg(C_TEXT_DIM)),
                Cell::from(err).style(Style::default().fg(C_TEXT_FAINT)),
            ])
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(16),
        Constraint::Length(38),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Min(0),
    ];

    let title = if app.search_query.is_empty() {
        "Logs d'exécution".to_string()
    } else {
        format!("Logs (Recherche: '{}')", app.search_query)
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(table_block(&title, app.filtered_logs.len()))
        .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ")
        .column_spacing(1);

    f.render_stateful_widget(table, area, &mut app.table_state);

    // Scrollbar interactive
    let mut scrollbar_state = ScrollbarState::default()
        .content_length(app.filtered_logs.len())
        .position(app.table_state.selected().unwrap_or(0));

    f.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓")),
        area.inner(&ratatui::layout::Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar_state,
    );
}

fn draw_artifacts_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header = header_row(&["  Statut", "Nom de l'artifact"]);
    let rows: Vec<Row> = app
        .filtered_artifacts
        .iter()
        .map(|art| {
            let status = art.status.as_deref().unwrap_or("—");
            let name = art.name.as_deref().unwrap_or("Inconnu");
            Row::new(vec![
                Cell::from(format!("  {} {}", status_icon(status), status))
                    .style(status_style(status)),
                Cell::from(name.to_string()).style(Style::default().fg(C_TEXT)),
            ])
            .height(1)
        })
        .collect();

    let table = Table::new(rows, [Constraint::Length(22), Constraint::Min(0)])
        .header(header)
        .block(table_block("Runtime Artifacts", app.filtered_artifacts.len()))
        .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_packages_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header = header_row(&["ID", "Nom", "Version", "Vendor"]);
    let rows: Vec<Row> = app
        .filtered_packages
        .iter()
        .map(|pkg| {
            Row::new(vec![
                Cell::from(pkg.id.as_deref().unwrap_or("—").to_string())
                    .style(Style::default().fg(C_ACCENT)),
                Cell::from(pkg.name.as_deref().unwrap_or("—").to_string())
                    .style(Style::default().fg(C_TEXT)),
                Cell::from(pkg.version.as_deref().unwrap_or("—").to_string())
                    .style(Style::default().fg(C_TEXT_DIM)),
                Cell::from(pkg.vendor.as_deref().unwrap_or("—").to_string())
                    .style(Style::default().fg(C_TEXT_FAINT)),
            ])
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(30),
        Constraint::Min(0),
        Constraint::Length(12),
        Constraint::Length(20),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(table_block("Integration Packages", app.filtered_packages.len()))
        .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_errors_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header = header_row(&["Artifact ID", "Date", "Message d'erreur"]);
    let rows: Vec<Row> = app
        .filtered_errors
        .iter()
        .map(|err| {
            let date_str = err
                .error_time
                .map(|d| d.format("%d/%m %H:%M").to_string())
                .unwrap_or("—".into());
            let msg = err
                .error_message
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(100)
                .collect::<String>();
            Row::new(vec![
                Cell::from(err.artifact_id.clone()).style(Style::default().fg(C_AMBER)),
                Cell::from(date_str).style(Style::default().fg(C_TEXT_DIM)),
                Cell::from(msg).style(Style::default().fg(C_RED)),
            ])
            .height(1)
        })
        .collect();

    if app.filtered_errors.is_empty() {
        let para = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  ● Aucune erreur",
                Style::default().fg(C_GREEN),
            )),
        ])
        .block(table_block("Erreurs de déploiement", 0));
        f.render_widget(para, area);
        return;
    }

    let widths = [
        Constraint::Length(35),
        Constraint::Length(14),
        Constraint::Min(0),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(table_block("Erreurs de déploiement", app.filtered_errors.len()))
        .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(table, area, &mut app.table_state);
}

// ─── Analytics Dashboard ─────────────────────────────────────────────────────

fn draw_analytics(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(rows[0]);

    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    // ── Gauge taux de succès ──
    let total = app.stats.total_logs.max(1) as f64;
    let completed = (app.stats.total_logs - app.stats.failed_logs).max(0) as f64;
    let ratio = (completed / total).clamp(0.0, 1.0);
    let pct = (ratio * 100.0) as u16;
    let gauge_color = if pct >= 90 { C_GREEN } else if pct >= 70 { C_AMBER } else { C_RED };
    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(" Taux de Succes ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(C_BORDER_ACTIVE))
                .style(Style::default().bg(C_SURFACE)),
        )
        .gauge_style(Style::default().fg(gauge_color).bg(C_SURFACE2))
        .ratio(ratio)
        .label(format!("{}%  ({} / {})", pct, completed as i64, app.stats.total_logs));
    f.render_widget(gauge, top[0]);

    // ── Top 5 artifacts en erreur ──
    let bars: Vec<Bar> = app
        .top_errors_barchart
        .iter()
        .map(|(label, val)| {
            Bar::default()
                .value(*val)
                .label(label.chars().take(12).collect::<String>().into())
                .style(Style::default().fg(C_RED))
        })
        .collect();
    let bc = BarChart::default()
        .block(
            Block::default()
                .title(" Top 5 Artifacts en Erreur ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(C_BORDER))
                .style(Style::default().bg(C_SURFACE)),
        )
        .data(BarGroup::default().bars(&bars))
        .bar_width(6)
        .bar_gap(1)
        .value_style(Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD));
    f.render_widget(bc, top[1]);

    // ── Sparkline volume global (12h) ──
    let spark = Sparkline::default()
        .block(
            Block::default()
                .title(" Volume d'activite global / heure (12h) ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(C_BORDER))
                .style(Style::default().bg(C_SURFACE)),
        )
        .data(&app.activity_sparkline)
        .style(Style::default().fg(C_BLUE));
    f.render_widget(spark, bottom[0]);

    // ── Répartition des statuts ──
    let status_icons = [
        ("COMPLETED", "●", C_GREEN),
        ("FAILED", "●", C_RED),
        ("PROCESSING", "◌", C_AMBER),
        ("STARTED", "●", C_BLUE),
    ];
    let mut lines = vec![Line::from(""), Line::from(Span::styled(" Repartition des statuts", Style::default().fg(C_TEXT_DIM).add_modifier(Modifier::BOLD))), Line::from("")];
    for (status, icon, color) in status_icons {
        let count = app
            .status_counts
            .iter()
            .find(|(s, _)| s == status)
            .map(|(_, c)| *c)
            .unwrap_or(0);
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(color)),
            Span::styled(format!("{:<12}", status), Style::default().fg(C_TEXT)),
            Span::styled(format!("{}", count), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));
    }
    let para = Paragraph::new(lines).block(
        Block::default()
            .title(" Statuts ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(C_BORDER))
            .style(Style::default().bg(C_SURFACE)),
    );
    f.render_widget(para, bottom[1]);
}

// ─── Footer ──────────────────────────────────────────────────────────────────

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    if app.search_active {
        let bar = Paragraph::new(Line::from(vec![
            Span::styled(
                " 🔍 Recherche : ",
                Style::default()
                    .fg(C_BG)
                    .bg(C_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", app.search_query),
                Style::default().fg(C_TEXT),
            ),
            Span::styled("█", Style::default().fg(C_ACCENT)),
        ]))
        .style(Style::default().bg(C_SURFACE2));
        f.render_widget(bar, area);
    } else {
        let mut spans: Vec<Span> = vec![Span::raw("  ")];
        let shortcuts = vec![
            ("↑↓ / j k", "naviguer"),
            ("Tab", "onglets"),
            ("type", "rechercher"),
            ("Esc", "effacer filtre"),
            ("f", "filtre erreurs"),
            ("+", "500 logs de plus"),
            ("r", "re-extraire"),
            ("q / Esc", "quitter"),
        ];

        for (key, action) in shortcuts {
            spans.push(Span::styled(
                format!(" {} ", key),
                Style::default().fg(C_BG).bg(C_ACCENT),
            ));
            spans.push(Span::styled(
                format!(" {}   ", action),
                Style::default().fg(C_TEXT_DIM),
            ));
        }

        if app.filter_failed {
            spans.push(Span::styled(
                " [F] FAILED SEULEMENT ",
                Style::default()
                    .fg(C_BG)
                    .bg(C_AMBER)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        let footer = Paragraph::new(Line::from(spans)).style(Style::default().bg(C_SURFACE));
        f.render_widget(footer, area);
    }
}

// ─── Overlay modal ───────────────────────────────────────────────────────────

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn draw_overlay(f: &mut Frame, app: &App, area: Rect) {
    let popup_area = ratatui::layout::Rect {
        x: area.x + area.width.saturating_sub(52) / 2,
        y: area.y + area.height.saturating_sub(7) / 2,
        width: 52.min(area.width),
        height: 7.min(area.height),
    };
    f.render_widget(Clear, popup_area);

    let (title, body_line, color, hint) = match &app.overlay {
        OverlayState::Running {
            message,
            spinner_tick,
        } => {
            let frame = SPINNER_FRAMES[(*spinner_tick as usize) % SPINNER_FRAMES.len()];
            (
                " Extraction SAP ",
                format!(" {}  {}", frame, message),
                C_ACCENT,
                "",
            )
        }
        OverlayState::Done { message } => (
            " Terminé ",
            format!(" ●  {}", message),
            C_GREEN,
            " Appuyez sur Entrée ",
        ),
        OverlayState::Error { message } => (
            " Erreur ",
            format!(" ●  {}", message),
            C_RED,
            " Appuyez sur Entrée ",
        ),
        OverlayState::ArtifactDetail { name, status, error } => {
            let popup_area = ratatui::layout::Rect {
                x: area.x + area.width.saturating_sub(70) / 2,
                y: area.y + area.height.saturating_sub(12) / 2,
                width: 70.min(area.width),
                height: 12.min(area.height),
            };
            f.render_widget(Clear, popup_area);
            let color = match status.as_str() {
                "STARTED" => C_GREEN,
                "ERROR" => C_RED,
                _ => C_AMBER,
            };
            let err_text = error.as_deref().unwrap_or("Aucune erreur enregistrée.");
            let body = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(" Statut  : ", Style::default().fg(C_TEXT_DIM)),
                    Span::styled(status.clone(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(""),
                Line::from(Span::styled(" Erreur  :", Style::default().fg(C_TEXT_DIM))),
                Line::from(Span::styled(format!(" {}", err_text), Style::default().fg(C_RED))),
                Line::from(""),
                Line::from(Span::styled(" Esc / Entrée pour fermer", Style::default().fg(C_TEXT_FAINT))),
            ])
            .block(
                Block::default()
                    .title(Span::styled(
                        format!(" {} ", name),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Double)
                    .border_style(Style::default().fg(color))
                    .style(Style::default().bg(C_SURFACE2)),
            )
            .wrap(Wrap { trim: false });
            f.render_widget(body, popup_area);
            return;
        }
        OverlayState::Hidden | OverlayState::LoadMore => return,
    };

    let popup = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            body_line,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(hint, Style::default().fg(C_TEXT_FAINT))),
    ])
    .block(
        Block::default()
            .title(Span::styled(
                title,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(color))
            .style(Style::default().bg(C_SURFACE2)),
    );

    f.render_widget(popup, popup_area);
}
