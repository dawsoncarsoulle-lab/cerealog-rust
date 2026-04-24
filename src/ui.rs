use crate::config::UserConfig;
use crate::db::{ArtifactView, ErrorView, LogView, PackageView};
use crate::models::{RefreshData, Stats};
use chrono::{Datelike, Days, NaiveDate};
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

// ─── Palette ─────────────────────────────────────────────────────────────────

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

// ─── Événements retournés à main ─────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum AppEvent {
    Quit,
    TriggerRefresh { full: bool },
    LoadMore,
    Continue,
}

// ─── Onglets ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Logs,
    Artifacts,
    Packages,
    DeployErrors,
    ExecErrors,
    Analytics,
}

impl Tab {
    fn all() -> &'static [Tab] {
        &[
            Tab::Logs,
            Tab::Artifacts,
            Tab::Packages,
            Tab::DeployErrors,
            Tab::ExecErrors,
            Tab::Analytics,
        ]
    }
    fn index(self) -> usize {
        Tab::all().iter().position(|&t| t == self).unwrap_or(0)
    }
}

// ─── Focus dans l'onglet Packages ─────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum PackageFocus {
    List,
    Artifacts,
    Activity,
}

// ─── État du calendrier ───────────────────────────────────────────────────────

#[derive(Clone)]
pub struct CalendarState {
    pub displayed_month: NaiveDate,
    pub cursor: NaiveDate,
    /// Phase 0 = choisir début, 1 = choisir fin
    pub phase: u8,
    pub date_start: Option<NaiveDate>,
    pub date_end: Option<NaiveDate>,
}

impl CalendarState {
    pub fn new() -> Self {
        let today = chrono::Local::now().date_naive();
        Self {
            displayed_month: today.with_day(1).unwrap_or(today),
            cursor: today,
            phase: 0,
            date_start: None,
            date_end: None,
        }
    }

    pub fn prev_month(&mut self) {
        let d = self.displayed_month;
        self.displayed_month = if d.month() == 1 {
            NaiveDate::from_ymd_opt(d.year() - 1, 12, 1).unwrap_or(d)
        } else {
            NaiveDate::from_ymd_opt(d.year(), d.month() - 1, 1).unwrap_or(d)
        };
    }

    pub fn next_month(&mut self) {
        let d = self.displayed_month;
        self.displayed_month = if d.month() == 12 {
            NaiveDate::from_ymd_opt(d.year() + 1, 1, 1).unwrap_or(d)
        } else {
            NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1).unwrap_or(d)
        };
    }

    pub fn prev_year(&mut self) {
        let d = self.displayed_month;
        self.displayed_month = NaiveDate::from_ymd_opt(d.year() - 1, d.month(), 1).unwrap_or(d);
    }

    pub fn next_year(&mut self) {
        let d = self.displayed_month;
        self.displayed_month = NaiveDate::from_ymd_opt(d.year() + 1, d.month(), 1).unwrap_or(d);
    }

    pub fn cursor_left(&mut self) {
        if let Some(d) = self.cursor.checked_sub_days(Days::new(1)) {
            self.cursor = d;
            self.sync_month();
        }
    }

    pub fn cursor_right(&mut self) {
        if let Some(d) = self.cursor.checked_add_days(Days::new(1)) {
            self.cursor = d;
            self.sync_month();
        }
    }

    pub fn cursor_up(&mut self) {
        if let Some(d) = self.cursor.checked_sub_days(Days::new(7)) {
            self.cursor = d;
            self.sync_month();
        }
    }

    pub fn cursor_down(&mut self) {
        if let Some(d) = self.cursor.checked_add_days(Days::new(7)) {
            self.cursor = d;
            self.sync_month();
        }
    }

    fn sync_month(&mut self) {
        if let Some(first) = NaiveDate::from_ymd_opt(self.cursor.year(), self.cursor.month(), 1) {
            self.displayed_month = first;
        }
    }

    /// Confirme la date sous le curseur. Retourne true si la sélection est complète.
    pub fn confirm(&mut self) -> bool {
        if self.phase == 0 {
            self.date_start = Some(self.cursor);
            self.date_end = None;
            self.phase = 1;
            false
        } else {
            let end = self.cursor;
            let start = self.date_start.unwrap_or(end);
            if end < start {
                self.date_start = Some(end);
                self.date_end = Some(start);
            } else {
                self.date_end = Some(end);
            }
            true
        }
    }
}

// ─── Overlay ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum OverlayState {
    Hidden,
    Done {
        message: String,
    },
    Error {
        message: String,
    },
    ArtifactDetail {
        name: String,
        status: String,
        error: Option<String>,
        configs: Vec<(String, String)>,
    },
    LogDetail {
        guid: String,
        status: String,
        date: String,
        flow: String,
        error: String,
    },
    Calendar(CalendarState),
}

// ─── App State ────────────────────────────────────────────────────────────────

pub struct App {
    pub active_tab: Tab,

    /// Un TableState par onglet (6 onglets)
    table_states: [TableState; 6],

    /// Focus interactif dans l'onglet Packages
    pub package_focus: PackageFocus,
    pub pkg_art_state: TableState,
    pub pkg_log_state: TableState,

    // Données brutes (reçues du worker)
    pub logs: Vec<LogView>,
    pub exec_errors: Vec<LogView>,
    pub artifacts: Vec<ArtifactView>,
    pub packages: Vec<PackageView>,
    pub deploy_errors: Vec<ErrorView>,
    pub configs: std::collections::HashMap<String, Vec<(String, String)>>,
    pub error_sparkline: Vec<u64>,
    pub error_barchart: Vec<(String, u64)>,
    pub activity_sparkline: Vec<u64>,
    pub top_errors_barchart: Vec<(String, u64)>,
    pub status_counts: Vec<(String, u64)>,

    // Données filtrées (affichées dans l'UI)
    pub filtered_logs: Vec<LogView>,
    pub filtered_exec_errors: Vec<LogView>,
    pub filtered_artifacts: Vec<ArtifactView>,
    pub filtered_packages: Vec<PackageView>,
    pub filtered_deploy_errors: Vec<ErrorView>,

    pub stats: Stats,
    pub search_query: String,
    pub search_active: bool,
    pub date_filter: Option<(NaiveDate, NaiveDate)>,
    pub logs_limit: u32,
    pub overlay: OverlayState,
    pub last_tick: Instant,
    pub last_refresh: Instant,
    pub user_config: UserConfig,

    /// Indique qu'un worker tourne en fond (affiche le spinner)
    pub refreshing: bool,
    pub refresh_spinner: u8,
}

impl App {
    pub fn new(user_config: UserConfig) -> Self {
        let logs_limit = user_config.logs_limit;
        let make_state = || {
            let mut s = TableState::default();
            s.select(Some(0));
            s
        };
        Self {
            active_tab: Tab::Logs,
            table_states: [
                make_state(),
                make_state(),
                make_state(),
                make_state(),
                make_state(),
                make_state(),
            ],
            package_focus: PackageFocus::List,
            pkg_art_state: make_state(),
            pkg_log_state: make_state(),
            logs: vec![],
            exec_errors: vec![],
            artifacts: vec![],
            packages: vec![],
            deploy_errors: vec![],
            configs: std::collections::HashMap::new(),
            error_sparkline: vec![],
            error_barchart: vec![],
            activity_sparkline: vec![],
            top_errors_barchart: vec![],
            status_counts: vec![],
            filtered_logs: vec![],
            filtered_exec_errors: vec![],
            filtered_artifacts: vec![],
            filtered_packages: vec![],
            filtered_deploy_errors: vec![],
            stats: Stats::default(),
            search_query: String::new(),
            search_active: false,
            date_filter: None,
            logs_limit,
            overlay: OverlayState::Hidden,
            last_tick: Instant::now(),
            last_refresh: Instant::now(),
            user_config,
            refreshing: false,
            refresh_spinner: 0,
        }
    }

    fn tab_index(tab: Tab) -> usize {
        match tab {
            Tab::Logs => 0,
            Tab::Artifacts => 1,
            Tab::Packages => 2,
            Tab::DeployErrors => 3,
            Tab::ExecErrors => 4,
            Tab::Analytics => 5,
        }
    }

    pub fn table_state(&mut self) -> &mut TableState {
        let i = Self::tab_index(self.active_tab);
        &mut self.table_states[i]
    }

    pub fn selected(&self) -> Option<usize> {
        let i = Self::tab_index(self.active_tab);
        self.table_states[i].selected()
    }

    fn select(&mut self, idx: Option<usize>) {
        let i = Self::tab_index(self.active_tab);
        self.table_states[i].select(idx);
    }

    /// Applique un RefreshData reçu du worker (zéro I/O, instantané).
    pub fn apply_refresh_data(&mut self, data: RefreshData) {
        self.logs = data.logs;
        self.exec_errors = data.exec_errors;
        self.artifacts = data.artifacts;
        self.packages = data.packages;
        self.deploy_errors = data.deploy_errors;
        self.configs = data.configs;
        self.stats = data.stats;
        self.error_sparkline = data.error_sparkline;
        self.error_barchart = data.error_barchart;
        self.activity_sparkline = data.activity_sparkline;
        self.top_errors_barchart = data.top_errors_barchart;
        self.status_counts = data.status_counts;
        self.last_refresh = Instant::now();
        self.apply_filters();
    }

    // ─── Filtres ──────────────────────────────────────────────────────────────

    pub fn apply_filters(&mut self) {
        let q = self.search_query.to_lowercase();
        let date_range = self.date_filter;

        let date_ok = |d: Option<chrono::NaiveDateTime>| -> bool {
            match (date_range, d) {
                (Some((start, end)), Some(dt)) => {
                    let day = dt.date();
                    day >= start && day <= end
                }
                (Some(_), None) => false,
                (None, _) => true,
            }
        };

        self.filtered_logs = self
            .logs
            .iter()
            .filter(|l| {
                let s = q.is_empty()
                    || l.status
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || l.error_message
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || l.message_guid
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || l.integration_flow_name
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q);
                s && date_ok(l.parsed_date)
            })
            .cloned()
            .collect();

        self.filtered_exec_errors = self
            .exec_errors
            .iter()
            .filter(|l| {
                let s = q.is_empty()
                    || l.message_guid
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || l.error_message
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || l.integration_flow_name
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q);
                s && date_ok(l.parsed_date)
            })
            .cloned()
            .collect();

        self.filtered_artifacts = self
            .artifacts
            .iter()
            .filter(|a| {
                q.is_empty()
                    || a.name.as_deref().unwrap_or("").to_lowercase().contains(&q)
                    || a.status
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || a.id.as_deref().unwrap_or("").to_lowercase().contains(&q)
            })
            .cloned()
            .collect();

        self.filtered_packages = self
            .packages
            .iter()
            .filter(|p| {
                q.is_empty()
                    || p.id.as_deref().unwrap_or("").to_lowercase().contains(&q)
                    || p.name.as_deref().unwrap_or("").to_lowercase().contains(&q)
                    || p.vendor
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || p.tags.as_deref().unwrap_or("").to_lowercase().contains(&q)
            })
            .cloned()
            .collect();

        self.filtered_deploy_errors = self
            .deploy_errors
            .iter()
            .filter(|e| {
                q.is_empty()
                    || e.artifact_id.to_lowercase().contains(&q)
                    || e.error_message
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
            })
            .cloned()
            .collect();

        let len = self.current_len();
        if let Some(i) = self.selected() {
            if len == 0 {
                self.select(None);
            } else if i >= len {
                self.select(Some(len - 1));
            }
        }
    }

    pub fn current_len(&self) -> usize {
        match self.active_tab {
            Tab::Logs => self.filtered_logs.len(),
            Tab::Artifacts => self.filtered_artifacts.len(),
            Tab::Packages => self.filtered_packages.len(),
            Tab::DeployErrors => self.filtered_deploy_errors.len(),
            Tab::ExecErrors => self.filtered_exec_errors.len(),
            Tab::Analytics => 0,
        }
    }

    pub fn next_row(&mut self) {
        if self.active_tab == Tab::Packages && self.package_focus != PackageFocus::List {
            match self.package_focus {
                PackageFocus::Artifacts => {
                    let i = self.pkg_art_state.selected().unwrap_or(0);
                    self.pkg_art_state.select(Some(i + 1));
                }
                PackageFocus::Activity => {
                    let i = self.pkg_log_state.selected().unwrap_or(0);
                    self.pkg_log_state.select(Some(i + 1));
                }
                _ => {}
            }
            return;
        }
        let len = self.current_len();
        if len == 0 {
            return;
        }
        let i = self.selected().unwrap_or(0);
        self.select(Some((i + 1).min(len - 1)));
    }

    pub fn prev_row(&mut self) {
        if self.active_tab == Tab::Packages && self.package_focus != PackageFocus::List {
            match self.package_focus {
                PackageFocus::Artifacts => {
                    let i = self.pkg_art_state.selected().unwrap_or(0);
                    self.pkg_art_state.select(Some(i.saturating_sub(1)));
                }
                PackageFocus::Activity => {
                    let i = self.pkg_log_state.selected().unwrap_or(0);
                    self.pkg_log_state.select(Some(i.saturating_sub(1)));
                }
                _ => {}
            }
            return;
        }
        let i = self.selected().unwrap_or(0);
        self.select(Some(i.saturating_sub(1)));
    }

    pub fn next_tab(&mut self) {
        self.package_focus = PackageFocus::List;
        let tabs = Tab::all();
        let i = (self.active_tab.index() + 1) % tabs.len();
        self.active_tab = tabs[i];
    }

    pub fn prev_tab(&mut self) {
        self.package_focus = PackageFocus::List;
        let tabs = Tab::all();
        let i = self.active_tab.index();
        self.active_tab = tabs[if i == 0 { tabs.len() - 1 } else { i - 1 }];
    }

    pub fn tick(&mut self) {
        if self.refreshing {
            self.refresh_spinner = self.refresh_spinner.wrapping_add(1);
        }
    }

    pub fn padded_sparkline(data: &[u64], num_points: usize) -> Vec<u64> {
        if data.len() >= num_points {
            data[data.len() - num_points..].to_vec()
        } else {
            let mut p = vec![0u64; num_points - data.len()];
            p.extend_from_slice(data);
            p
        }
    }
}

// ─── Entrée TUI ───────────────────────────────────────────────────────────────

pub fn run_tui(
    app: &mut App,
    rx: &mut tokio::sync::mpsc::Receiver<RefreshData>,
) -> anyhow::Result<AppEvent> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tick_rate = Duration::from_millis(120);
    let result = run_loop(&mut terminal, app, tick_rate, rx);

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
    rx: &mut tokio::sync::mpsc::Receiver<RefreshData>,
) -> anyhow::Result<AppEvent> {
    loop {
        // ── Consommer les données du worker (zéro-blocking) ───────────────────
        while let Ok(data) = rx.try_recv() {
            app.apply_refresh_data(data);
            app.refreshing = false;
        }

        terminal.draw(|f| draw(f, app))?;

        let timeout = tick_rate
            .checked_sub(app.last_tick.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(AppEvent::Quit);
                }

                // ── Overlay Calendrier ────────────────────────────────────────
                if let OverlayState::Calendar(ref mut cal) = app.overlay {
                    match key.code {
                        KeyCode::Esc => {
                            app.overlay = OverlayState::Hidden;
                        }
                        KeyCode::Left => {
                            if key.modifiers.contains(KeyModifiers::SHIFT) {
                                cal.prev_month();
                            } else {
                                cal.cursor_left();
                            }
                        }
                        KeyCode::Right => {
                            if key.modifiers.contains(KeyModifiers::SHIFT) {
                                cal.next_month();
                            } else {
                                cal.cursor_right();
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => cal.cursor_up(),
                        KeyCode::Down | KeyCode::Char('j') => cal.cursor_down(),
                        KeyCode::Char('h') => cal.cursor_left(),
                        KeyCode::Char('l') => cal.cursor_right(),
                        KeyCode::Char('c') => {
                            app.overlay = OverlayState::Hidden;
                            app.date_filter = None;
                            app.apply_filters();
                        }
                        KeyCode::Enter => {
                            let done = cal.confirm();
                            if done {
                                let start = cal.date_start.unwrap();
                                let end = cal.date_end.unwrap();
                                app.date_filter = Some((start, end));
                                app.overlay = OverlayState::Hidden;
                                app.apply_filters();
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // ── Overlays génériques (Done / Error / Detail) ───────────────
                match &app.overlay {
                    OverlayState::Done { .. }
                    | OverlayState::Error { .. }
                    | OverlayState::ArtifactDetail { .. }
                    | OverlayState::LogDetail { .. } => {
                        if matches!(key.code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q')) {
                            app.overlay = OverlayState::Hidden;
                        }
                        continue;
                    }
                    _ => {}
                }

                // ── Mode recherche ────────────────────────────────────────────
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

                // ── Raccourcis globaux ────────────────────────────────────────
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        if app.active_tab == Tab::Packages
                            && app.package_focus != PackageFocus::List
                        {
                            app.package_focus = PackageFocus::List;
                        } else {
                            return Ok(AppEvent::Quit);
                        }
                    }

                    KeyCode::Tab => app.next_tab(),
                    KeyCode::BackTab => app.prev_tab(),

                    KeyCode::Right
                        if app.active_tab != Tab::Packages
                            || app.package_focus == PackageFocus::List =>
                    {
                        app.next_tab();
                    }
                    KeyCode::Left
                        if app.active_tab != Tab::Packages
                            || app.package_focus == PackageFocus::List =>
                    {
                        app.prev_tab();
                    }

                    KeyCode::Down | KeyCode::Char('j') => app.next_row(),
                    KeyCode::Up | KeyCode::Char('k') => app.prev_row(),

                    KeyCode::Char('r') if !app.refreshing => {
                        app.refreshing = true;
                        return Ok(AppEvent::TriggerRefresh { full: false });
                    }

                    KeyCode::Char('R') if !app.refreshing => {
                        app.refreshing = true;
                        return Ok(AppEvent::TriggerRefresh { full: true });
                    }

                    KeyCode::Char('+') => {
                        app.logs_limit += 500;
                        return Ok(AppEvent::LoadMore);
                    }

                    KeyCode::Char('d') => {
                        app.overlay = OverlayState::Calendar(CalendarState::new());
                    }

                    KeyCode::Char('D') => {
                        app.date_filter = None;
                        app.apply_filters();
                    }

                    // Entrée : focus Packages ou overlay détail
                    KeyCode::Enter if app.active_tab == Tab::Packages => match app.package_focus {
                        PackageFocus::List => {
                            app.package_focus = PackageFocus::Artifacts;
                            app.pkg_art_state.select(Some(0));
                        }
                        PackageFocus::Artifacts => {
                            app.package_focus = PackageFocus::Activity;
                            app.pkg_log_state.select(Some(0));
                        }
                        PackageFocus::Activity => {
                            app.package_focus = PackageFocus::List;
                        }
                    },

                    KeyCode::Enter if app.active_tab == Tab::Artifacts => {
                        if let Some(i) = app.selected() {
                            if let Some(art) = app.filtered_artifacts.get(i) {
                                let error = app
                                    .deploy_errors
                                    .iter()
                                    .find(|e| Some(e.artifact_id.as_str()) == art.id.as_deref())
                                    .and_then(|e| e.error_message.clone());

                                let art_configs = app
                                    .configs
                                    .get(art.id.as_deref().unwrap_or(""))
                                    .cloned()
                                    .unwrap_or_default();

                                app.overlay = OverlayState::ArtifactDetail {
                                    name: art.name.clone().unwrap_or_default(),
                                    status: art.status.clone().unwrap_or_default(),
                                    error,
                                    configs: art_configs,
                                };
                            }
                        }
                    }

                    KeyCode::Enter
                        if app.active_tab == Tab::Logs || app.active_tab == Tab::ExecErrors =>
                    {
                        let log_opt = if app.active_tab == Tab::Logs {
                            app.selected().and_then(|i| app.filtered_logs.get(i))
                        } else {
                            app.selected().and_then(|i| app.filtered_exec_errors.get(i))
                        };
                        if let Some(log) = log_opt {
                            app.overlay = OverlayState::LogDetail {
                                guid: log.message_guid.clone().unwrap_or_else(|| "—".to_string()),
                                status: log.status.clone().unwrap_or_else(|| "—".to_string()),
                                date: log
                                    .parsed_date
                                    .map(|d| d.format("%d/%m/%Y %H:%M:%S").to_string())
                                    .unwrap_or_else(|| "—".to_string()),
                                flow: log
                                    .integration_flow_name
                                    .clone()
                                    .unwrap_or_else(|| "—".to_string()),
                                error: log
                                    .error_message
                                    .clone()
                                    .unwrap_or_else(|| "Aucune information d'erreur.".to_string()),
                            };
                        }
                    }

                    KeyCode::Char(c)
                        if app.active_tab != Tab::Analytics
                            && app.package_focus == PackageFocus::List =>
                    {
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

        if app.last_refresh.elapsed().as_secs() >= 295 && !app.refreshing {
            app.refreshing = true;
            return Ok(AppEvent::Continue);
        }
    }
}

// ─── Rendu principal ──────────────────────────────────────────────────────────

fn draw(f: &mut Frame, app: &mut App) {
    let size = f.size();
    f.render_widget(Block::default().style(Style::default().bg(C_BG)), size);

    let is_analytics = app.active_tab == Tab::Analytics;

    let chunks = if is_analytics {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(10),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(size)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(size)
    };

    draw_header(f, app, chunks[0]);

    if is_analytics {
        draw_stats_and_charts(f, app, chunks[1]);
        draw_body(f, app, chunks[2]);
        draw_footer(f, app, chunks[3]);
    } else {
        draw_body(f, app, chunks[1]);
        draw_footer(f, app, chunks[2]);
    }

    match &app.overlay {
        OverlayState::Hidden => {}
        OverlayState::Calendar(_) => draw_overlay_calendar(f, app, size),
        _ => draw_overlay(f, app, size),
    }
}

// ─── Header ───────────────────────────────────────────────────────────────────

fn tab_label(tab: Tab, app: &App) -> Line<'static> {
    let (name, counts, is_alert) = match tab {
        Tab::Logs => (
            "Logs",
            Some((app.filtered_logs.len(), app.logs.len())),
            false,
        ),
        Tab::Artifacts => (
            "Artifacts",
            Some((app.filtered_artifacts.len(), app.artifacts.len())),
            false,
        ),
        Tab::Packages => (
            "Packages",
            Some((app.filtered_packages.len(), app.packages.len())),
            false,
        ),
        Tab::DeployErrors => (
            "Err.Déploi.",
            Some((app.filtered_deploy_errors.len(), app.deploy_errors.len())),
            !app.filtered_deploy_errors.is_empty(),
        ),
        Tab::ExecErrors => (
            "Err.Exéc.",
            Some((app.filtered_exec_errors.len(), app.exec_errors.len())),
            !app.filtered_exec_errors.is_empty(),
        ),
        Tab::Analytics => ("Analytics", None, false),
    };

    let badge_color = if is_alert { C_RED } else { C_TEXT_FAINT };
    let mut spans = vec![Span::raw(format!("  {} ", name))];

    if let Some((filtered, total)) = counts {
        let has_filter = !app.search_query.is_empty() || app.date_filter.is_some();
        let badge = if has_filter {
            format!("[{}/{}]  ", filtered, total)
        } else {
            format!("[{}]  ", total)
        };
        spans.push(Span::styled(badge, Style::default().fg(badge_color)));
    } else {
        spans.push(Span::raw("  "));
    }

    Line::from(spans)
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(36), Constraint::Min(0)])
        .split(area);

    let age = app.last_refresh.elapsed().as_secs();
    let freshness = if age < 60 {
        format!("{}s", age)
    } else {
        format!("{}m{}s", age / 60, age % 60)
    };
    let freshness_color = if age > 300 { C_RED } else { C_TEXT_FAINT };

    const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let spinner_str = if app.refreshing {
        format!(
            "{} ",
            SPINNER[(app.refresh_spinner as usize) % SPINNER.len()]
        )
    } else {
        String::new()
    };

    let cal_indicator = if let Some((s, e)) = app.date_filter {
        format!("📅 {}→{}  ", s.format("%d/%m"), e.format("%d/%m"))
    } else {
        String::new()
    };

    let title_line = Line::from(vec![
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
        Span::styled(spinner_str, Style::default().fg(C_ACCENT)),
        Span::styled(cal_indicator, Style::default().fg(C_BLUE)),
    ]);

    let title = Paragraph::new(title_line)
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

// ─── Stats & Charts (Analytics uniquement) ────────────────────────────────────

fn draw_stats_and_charts(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

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
        f.render_widget(
            Paragraph::new(content).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(C_BORDER))
                    .style(Style::default().bg(C_SURFACE)),
            ),
            rect,
        );
    }

    let chart_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(cols[1]);

    let sparkline_data = App::padded_sparkline(&app.error_sparkline, 24);
    f.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .title(" Erreurs / heure (24h) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(C_BORDER))
                    .border_type(BorderType::Rounded),
            )
            .data(&sparkline_data)
            .style(Style::default().fg(C_RED)),
        chart_rows[0],
    );

    let bars: Vec<Bar> = app
        .error_barchart
        .iter()
        .map(|(label, val)| Bar::default().value(*val).label(label.as_str().into()))
        .collect();
    f.render_widget(
        BarChart::default()
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
            .value_style(Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)),
        chart_rows[1],
    );
}

// ─── Body ─────────────────────────────────────────────────────────────────────

fn draw_body(f: &mut Frame, app: &mut App, area: Rect) {
    match app.active_tab {
        Tab::Logs => draw_logs_master_detail(f, app, area),
        Tab::Artifacts => draw_artifacts_table(f, app, area),
        Tab::Packages => draw_packages_master_detail(f, app, area),
        Tab::DeployErrors => draw_deploy_errors_table(f, app, area),
        Tab::ExecErrors => draw_exec_errors_table(f, app, area),
        Tab::Analytics => draw_analytics(f, app, area),
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn highlight_text(text: &str, query: &str, base_style: Style) -> Line<'static> {
    let hl = Style::default()
        .fg(C_BG)
        .bg(C_GREEN)
        .add_modifier(Modifier::BOLD);

    if query.is_empty() {
        return Line::from(Span::styled(text.to_string(), base_style));
    }

    let lt = text.to_lowercase();
    let lq = query.to_lowercase();
    let mut spans = Vec::new();
    let mut last = 0;

    for (start, part) in lt.match_indices(&lq) {
        if start > last {
            spans.push(Span::styled(text[last..start].to_string(), base_style));
        }
        spans.push(Span::styled(
            text[start..start + part.len()].to_string(),
            hl,
        ));
        last = start + part.len();
    }
    if last < text.len() {
        spans.push(Span::styled(text[last..].to_string(), base_style));
    }
    Line::from(spans)
}

fn table_block(
    title: &str,
    filtered: usize,
    total: usize,
    search: &str,
    has_date: bool,
) -> Block<'static> {
    let count = if search.is_empty() && !has_date {
        format!("({}) ", total)
    } else {
        format!("({}/{}) ", filtered, total)
    };
    Block::default()
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", title),
                Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(count, Style::default().fg(C_TEXT_DIM)),
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
        "FAILED" | "ERROR" => Style::default().fg(C_RED).add_modifier(Modifier::BOLD),
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

fn render_scrollbar(f: &mut Frame, area: Rect, len: usize, selected: usize) {
    let mut state = ScrollbarState::default()
        .content_length(len)
        .position(selected);
    f.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓")),
        area.inner(&ratatui::layout::Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut state,
    );
}

fn build_log_row(log: &LogView, query: &str) -> Row<'static> {
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
    let flow = log
        .integration_flow_name
        .as_deref()
        .unwrap_or("—")
        .chars()
        .take(22)
        .collect::<String>();
    let err = log
        .error_message
        .as_deref()
        .unwrap_or("")
        .chars()
        .take(50)
        .collect::<String>();

    Row::new(vec![
        Cell::from(highlight_text(
            &format!("  {} {}", status_icon(status), status),
            query,
            status_style(status),
        )),
        Cell::from(highlight_text(guid, query, Style::default().fg(C_ACCENT))),
        Cell::from(highlight_text(&flow, query, Style::default().fg(C_BLUE))),
        Cell::from(highlight_text(
            &date_str,
            query,
            Style::default().fg(C_TEXT_DIM),
        )),
        Cell::from(highlight_text(
            &time_str,
            query,
            Style::default().fg(C_TEXT_DIM),
        )),
        Cell::from(highlight_text(
            &err,
            query,
            Style::default().fg(C_TEXT_FAINT),
        )),
    ])
    .height(1)
}

fn log_detail_text(log: Option<&LogView>) -> String {
    log.map(|l| {
        let date = l
            .parsed_date
            .map(|d| d.format("%d/%m/%Y %H:%M:%S").to_string())
            .unwrap_or_default();
        format!(
            "GUID    : {}\nDate    : {}\nStatut  : {}\nFlow    : {}\n\nErreur  :\n{}\n\n[Entrée] pour afficher l'erreur complète",
            l.message_guid.as_deref().unwrap_or("—"),
            date,
            l.status.as_deref().unwrap_or("—"),
            l.integration_flow_name.as_deref().unwrap_or("—"),
            l.error_message.as_deref().unwrap_or("Aucune information d'erreur."),
        )
    })
    .unwrap_or_else(|| "Sélectionnez une ligne · [Entrée] pour le détail complet".to_string())
}

fn log_border_color(log: Option<&LogView>) -> Color {
    log.and_then(|l| l.status.as_deref())
        .map(|s| match s {
            "FAILED" => C_RED,
            "COMPLETED" => C_GREEN,
            _ => C_BORDER,
        })
        .unwrap_or(C_BORDER)
}

// ─── Logs ─────────────────────────────────────────────────────────────────────

fn draw_logs_master_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    let header = header_row(&[
        "  Statut",
        "ID Message",
        "Flow",
        "Date",
        "Heure",
        "Aperçu erreur",
    ]);
    let query = app.search_query.clone();
    let has_date = app.date_filter.is_some();
    let rows: Vec<Row> = app
        .filtered_logs
        .iter()
        .map(|l| build_log_row(l, &query))
        .collect();
    let total = app.logs.len();
    let filtered = app.filtered_logs.len();
    let title = if query.is_empty() {
        "Logs d'exécution".to_string()
    } else {
        format!("Logs · '{}'", query)
    };
    let block = table_block(&title, filtered, total, &query, has_date);
    let selected = app.selected().unwrap_or(0);
    let widths = [
        Constraint::Length(16),
        Constraint::Length(38),
        Constraint::Length(24),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Min(0),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ")
        .column_spacing(1);
    f.render_stateful_widget(table, chunks[0], &mut app.table_states[0]);
    render_scrollbar(f, chunks[0], filtered, selected);

    let selected_log = app.selected().and_then(|i| app.filtered_logs.get(i));
    let border_color = log_border_color(selected_log);
    let detail = Paragraph::new(log_detail_text(selected_log))
        .block(
            Block::default()
                .title(Span::styled(
                    " Vue Détaillée — [Entrée] pour tout afficher ",
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

// ─── Artifacts ────────────────────────────────────────────────────────────────

fn draw_artifacts_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header = header_row(&["  Statut", "ID", "Nom de l'artifact", "Package"]);
    let query = app.search_query.clone();
    let has_date = app.date_filter.is_some();
    let rows: Vec<Row> = app
        .filtered_artifacts
        .iter()
        .map(|art| {
            let status = art.status.as_deref().unwrap_or("—");
            Row::new(vec![
                Cell::from(highlight_text(
                    &format!("  {} {}", status_icon(status), status),
                    &query,
                    status_style(status),
                )),
                Cell::from(highlight_text(
                    art.id.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_TEXT_FAINT),
                )),
                Cell::from(highlight_text(
                    art.name.as_deref().unwrap_or("Inconnu"),
                    &query,
                    Style::default().fg(C_TEXT),
                )),
                Cell::from(highlight_text(
                    art.package_id.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_ACCENT2),
                )),
            ])
            .height(1)
        })
        .collect();

    let total = app.artifacts.len();
    let filtered = app.filtered_artifacts.len();
    let selected = app.selected().unwrap_or(0);
    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(30),
            Constraint::Min(0),
            Constraint::Length(28),
        ],
    )
    .header(header)
    .block(table_block(
        "Runtime Artifacts",
        filtered,
        total,
        &query,
        has_date,
    ))
    .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ");
    f.render_stateful_widget(table, area, &mut app.table_states[1]);
    render_scrollbar(f, area, filtered, selected);
}

// ─── Packages ─────────────────────────────────────────────────────────────────

fn draw_packages_master_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // ── Liste principale ──────────────────────────────────────────────────────
    let header = header_row(&["ID", "Nom", "Version", "Tags", "Vendor"]);
    let query = app.search_query.clone();
    let has_date = app.date_filter.is_some();
    let pkg_rows: Vec<Row> = app
        .filtered_packages
        .iter()
        .map(|pkg| {
            Row::new(vec![
                Cell::from(highlight_text(
                    pkg.id.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_ACCENT),
                )),
                Cell::from(highlight_text(
                    pkg.name.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_TEXT),
                )),
                Cell::from(highlight_text(
                    pkg.version.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_TEXT_DIM),
                )),
                Cell::from(highlight_text(
                    pkg.tags.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_AMBER),
                )),
                Cell::from(highlight_text(
                    pkg.vendor.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_TEXT_FAINT),
                )),
            ])
            .height(1)
        })
        .collect();

    let total = app.packages.len();
    let filtered = app.filtered_packages.len();
    let list_border = if app.package_focus == PackageFocus::List {
        C_BORDER_ACTIVE
    } else {
        C_BORDER
    };
    let selected_pkg_idx = app.selected().unwrap_or(0);

    let pkg_table = Table::new(
        pkg_rows,
        [
            Constraint::Length(30),
            Constraint::Min(0),
            Constraint::Length(10),
            Constraint::Length(35),
            Constraint::Length(15),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(Line::from(vec![
                Span::styled(
                    " Integration Packages ",
                    Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    if query.is_empty() && !has_date {
                        format!("({}) ", total)
                    } else {
                        format!("({}/{}) ", filtered, total)
                    },
                    Style::default().fg(C_TEXT_DIM),
                ),
            ]))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(list_border))
            .style(Style::default().bg(C_SURFACE)),
    )
    .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ");

    f.render_stateful_widget(pkg_table, chunks[0], &mut app.table_states[2]);
    render_scrollbar(f, chunks[0], filtered, selected_pkg_idx);

    // ── Panneaux de détail ────────────────────────────────────────────────────
    let detail_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[1]);

    let selected_pkg = app
        .selected()
        .and_then(|i| app.filtered_packages.get(i).cloned());
    let pkg_id = selected_pkg
        .as_ref()
        .and_then(|p| p.id.as_deref())
        .unwrap_or("")
        .to_string();

    let pkg_artifacts: Vec<ArtifactView> = app
        .artifacts
        .iter()
        .filter(|a| a.package_id.as_deref() == Some(pkg_id.as_str()))
        .cloned()
        .collect();

    let valid_ids: std::collections::HashSet<String> = pkg_artifacts
        .iter()
        .flat_map(|a| {
            let mut v = vec![];
            if let Some(id) = &a.id {
                v.push(id.clone());
            }
            if let Some(n) = &a.name {
                v.push(n.clone());
            }
            v
        })
        .collect();

    let pkg_logs: Vec<LogView> = app
        .logs
        .iter()
        .filter(|l| {
            l.integration_flow_name
                .as_ref()
                .map(|f| valid_ids.contains(f))
                .unwrap_or(false)
        })
        .take(50)
        .cloned()
        .collect();

    let art_border = if app.package_focus == PackageFocus::Artifacts {
        C_ACCENT
    } else {
        C_BORDER
    };
    let log_border = if app.package_focus == PackageFocus::Activity {
        C_ACCENT
    } else {
        C_BORDER
    };

    let focus_hint = match app.package_focus {
        PackageFocus::List => " [Entrée] → naviguer dans les panneaux ",
        PackageFocus::Artifacts => " focus · [↑↓] naviguer · [Entrée] → Activité ",
        PackageFocus::Activity => " focus · [↑↓] naviguer · [Entrée] → Liste  · [Esc] ",
    };

    // Artifacts du package
    let art_rows: Vec<Row> = pkg_artifacts
        .iter()
        .map(|art| {
            let status = art.status.as_deref().unwrap_or("—");
            Row::new(vec![
                Cell::from(format!("  {} {}", status_icon(status), status))
                    .style(status_style(status)),
                Cell::from(art.name.as_deref().unwrap_or("Inconnu").to_string())
                    .style(Style::default().fg(C_TEXT)),
            ])
        })
        .collect();

    let mut art_state = app.pkg_art_state.clone();
    f.render_stateful_widget(
        Table::new(art_rows, [Constraint::Length(15), Constraint::Min(0)])
            .header(header_row(&["  Statut", "Artifact"]))
            .block(
                Block::default()
                    .title(Span::styled(
                        format!(" Artifacts inclus ({}) ", pkg_artifacts.len()),
                        Style::default().fg(C_ACCENT2).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(art_border))
                    .style(Style::default().bg(C_SURFACE2)),
            )
            .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ "),
        detail_chunks[0],
        &mut art_state,
    );
    app.pkg_art_state = art_state;

    // Activité récente
    let log_rows: Vec<Row> = pkg_logs
        .iter()
        .map(|log| {
            let status = log.status.as_deref().unwrap_or("—");
            Row::new(vec![
                Cell::from(format!("  {} {}", status_icon(status), status))
                    .style(status_style(status)),
                Cell::from(
                    log.parsed_date
                        .map(|d| d.format("%d/%m %H:%M").to_string())
                        .unwrap_or_default(),
                )
                .style(Style::default().fg(C_TEXT_DIM)),
                Cell::from(
                    log.integration_flow_name
                        .as_deref()
                        .unwrap_or("—")
                        .chars()
                        .take(20)
                        .collect::<String>(),
                )
                .style(Style::default().fg(C_BLUE)),
                Cell::from(
                    log.error_message
                        .as_deref()
                        .unwrap_or("")
                        .chars()
                        .take(40)
                        .collect::<String>(),
                )
                .style(Style::default().fg(C_TEXT_FAINT)),
            ])
        })
        .collect();

    let mut log_state = app.pkg_log_state.clone();
    f.render_stateful_widget(
        Table::new(
            log_rows,
            [
                Constraint::Length(15),
                Constraint::Length(14),
                Constraint::Length(22),
                Constraint::Min(0),
            ],
        )
        .header(header_row(&[
            "  Statut",
            "Date",
            "Source",
            "Erreur / Détail",
        ]))
        .block(
            Block::default()
                .title(Line::from(vec![
                    Span::styled(
                        format!(" Activité récente ({}) ", pkg_logs.len()),
                        Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(focus_hint, Style::default().fg(C_TEXT_FAINT)),
                ]))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(log_border))
                .style(Style::default().bg(C_SURFACE2)),
        )
        .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ "),
        detail_chunks[1],
        &mut log_state,
    );
    app.pkg_log_state = log_state;
}

// ─── Erreurs de déploiement ───────────────────────────────────────────────────

fn draw_deploy_errors_table(f: &mut Frame, app: &mut App, area: Rect) {
    if app.filtered_deploy_errors.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  ● Aucune erreur de déploiement",
                    Style::default().fg(C_GREEN),
                )),
            ])
            .block(table_block("Erreurs de Déploiement BTP", 0, 0, "", false)),
            area,
        );
        return;
    }

    let header = header_row(&["Artifact ID", "Date", "Message d'erreur"]);
    let query = app.search_query.clone();
    let has_date = app.date_filter.is_some();
    let rows: Vec<Row> = app
        .filtered_deploy_errors
        .iter()
        .map(|err| {
            Row::new(vec![
                Cell::from(highlight_text(
                    &err.artifact_id,
                    &query,
                    Style::default().fg(C_AMBER),
                )),
                Cell::from(
                    err.error_time
                        .map(|d| d.format("%d/%m %H:%M").to_string())
                        .unwrap_or("—".into()),
                )
                .style(Style::default().fg(C_TEXT_DIM)),
                Cell::from(highlight_text(
                    &err.error_message
                        .as_deref()
                        .unwrap_or("")
                        .chars()
                        .take(100)
                        .collect::<String>(),
                    &query,
                    Style::default().fg(C_RED),
                )),
            ])
            .height(1)
        })
        .collect();

    let total = app.deploy_errors.len();
    let filtered = app.filtered_deploy_errors.len();
    let selected = app.selected().unwrap_or(0);
    let table = Table::new(
        rows,
        [
            Constraint::Length(35),
            Constraint::Length(14),
            Constraint::Min(0),
        ],
    )
    .header(header)
    .block(table_block(
        "Erreurs de Déploiement BTP",
        filtered,
        total,
        &query,
        has_date,
    ))
    .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ");
    f.render_stateful_widget(table, area, &mut app.table_states[3]);
    render_scrollbar(f, area, filtered, selected);
}

// ─── Erreurs d'exécution ──────────────────────────────────────────────────────

fn draw_exec_errors_table(f: &mut Frame, app: &mut App, area: Rect) {
    if app.filtered_exec_errors.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  ● Aucune erreur d'exécution — tous les flux sont opérationnels",
                    Style::default().fg(C_GREEN),
                )),
            ])
            .block(table_block(
                "Erreurs d'Exécution — MPL FAILED",
                0,
                0,
                "",
                false,
            )),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    let header = header_row(&[
        "  Statut",
        "ID Message",
        "Flow",
        "Date",
        "Heure",
        "Aperçu erreur",
    ]);
    let query = app.search_query.clone();
    let has_date = app.date_filter.is_some();
    let rows: Vec<Row> = app
        .filtered_exec_errors
        .iter()
        .map(|l| build_log_row(l, &query))
        .collect();
    let total = app.exec_errors.len();
    let filtered = app.filtered_exec_errors.len();
    let selected = app.selected().unwrap_or(0);

    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(38),
            Constraint::Length(24),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Min(0),
        ],
    )
    .header(header)
    .block(table_block(
        "Erreurs d'Exécution — MPL FAILED",
        filtered,
        total,
        &query,
        has_date,
    ))
    .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ")
    .column_spacing(1);

    f.render_stateful_widget(table, chunks[0], &mut app.table_states[4]);
    render_scrollbar(f, chunks[0], filtered, selected);

    let selected_log = app.selected().and_then(|i| app.filtered_exec_errors.get(i));
    let detail = Paragraph::new(log_detail_text(selected_log))
        .block(
            Block::default()
                .title(Span::styled(
                    " Détail de l'erreur — [Entrée] pour tout afficher ",
                    Style::default().fg(C_RED).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(C_RED))
                .style(Style::default().bg(C_SURFACE)),
        )
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(C_TEXT));
    f.render_widget(detail, chunks[1]);
}

// ─── Analytics ────────────────────────────────────────────────────────────────

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

    let total = app.stats.total_logs.max(1) as f64;
    let completed = (app.stats.total_logs - app.stats.failed_logs).max(0) as f64;
    let ratio = (completed / total).clamp(0.0, 1.0);
    let pct = (ratio * 100.0) as u16;
    let gauge_color = if pct >= 90 {
        C_GREEN
    } else if pct >= 70 {
        C_AMBER
    } else {
        C_RED
    };

    f.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .title(" Taux de Succès ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(C_BORDER_ACTIVE))
                    .style(Style::default().bg(C_SURFACE)),
            )
            .gauge_style(Style::default().fg(gauge_color).bg(C_SURFACE2))
            .ratio(ratio)
            .label(format!(
                "{}%  ({} / {})",
                pct, completed as i64, app.stats.total_logs
            )),
        top[0],
    );

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
    f.render_widget(
        BarChart::default()
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
            .value_style(Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)),
        top[1],
    );

    let activity_data = App::padded_sparkline(&app.activity_sparkline, 12);
    f.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .title(" Volume d'activité global / heure (12h) ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(C_BORDER))
                    .style(Style::default().bg(C_SURFACE)),
            )
            .data(&activity_data)
            .style(Style::default().fg(C_BLUE)),
        bottom[0],
    );

    let status_entries = [
        ("COMPLETED", "●", C_GREEN),
        ("FAILED", "●", C_RED),
        ("PROCESSING", "◌", C_AMBER),
        ("STARTED", "●", C_BLUE),
        ("ERROR", "●", C_RED),
    ];
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " Répartition des statuts",
            Style::default().fg(C_TEXT_DIM).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for (status, icon, color) in status_entries {
        let count = app
            .status_counts
            .iter()
            .find(|(s, _)| s == status)
            .map(|(_, c)| *c)
            .unwrap_or(0);
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(color)),
            Span::styled(format!("{:<12}", status), Style::default().fg(C_TEXT)),
            Span::styled(
                format!("{}", count),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(""));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(" Statuts ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(C_BORDER))
                .style(Style::default().bg(C_SURFACE)),
        ),
        bottom[1],
    );
}

// ─── Footer ───────────────────────────────────────────────────────────────────

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    if app.search_active {
        f.render_widget(
            Paragraph::new(Line::from(vec![
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
            .style(Style::default().bg(C_SURFACE2)),
            area,
        );
        return;
    }

    let mut spans: Vec<Span> = vec![Span::raw("  ")];
    let shortcuts = [
        ("↑↓/jk", "nav"),
        ("Tab", "onglet"),
        ("Enter", "détail"),
        ("type", "chercher"),
        ("Esc", "effacer"),
        ("d", "calendrier"),
        ("D", "effacer date"),
        ("+", "500 logs"),
        ("r", "refresh"),
        ("q", "quitter"),
    ];
    for (key, action) in shortcuts {
        spans.push(Span::styled(
            format!(" {} ", key),
            Style::default().fg(C_BG).bg(C_ACCENT),
        ));
        spans.push(Span::styled(
            format!(" {}  ", action),
            Style::default().fg(C_TEXT_DIM),
        ));
    }
    if app.date_filter.is_some() {
        spans.push(Span::styled(
            " 📅 DATE ACTIVE ",
            Style::default()
                .fg(C_BG)
                .bg(C_BLUE)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if app.refreshing {
        spans.push(Span::styled(
            " ⟳ REFRESH… ",
            Style::default()
                .fg(C_BG)
                .bg(C_ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(C_SURFACE)),
        area,
    );
}

// ─── Overlay Calendrier ───────────────────────────────────────────────────────

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(30)
}

fn month_name(m: u32) -> &'static str {
    match m {
        1 => "Janvier",
        2 => "Février",
        3 => "Mars",
        4 => "Avril",
        5 => "Mai",
        6 => "Juin",
        7 => "Juillet",
        8 => "Août",
        9 => "Septembre",
        10 => "Octobre",
        11 => "Novembre",
        _ => "Décembre",
    }
}

fn draw_overlay_calendar(f: &mut Frame, app: &App, area: Rect) {
    let cal = if let OverlayState::Calendar(ref c) = app.overlay {
        c
    } else {
        return;
    };

    let popup_w = 48u16;
    let popup_h = 20u16;
    let popup_area = Rect {
        x: area.x + area.width.saturating_sub(popup_w) / 2,
        y: area.y + area.height.saturating_sub(popup_h) / 2,
        width: popup_w.min(area.width),
        height: popup_h.min(area.height),
    };
    f.render_widget(Clear, popup_area);

    let title_color = if cal.phase == 0 { C_BLUE } else { C_ACCENT };
    let phase_label = if cal.phase == 0 {
        "  ① Choisissez la date de DÉBUT"
    } else {
        "  ② Choisissez la date de FIN  "
    };

    let first_day = cal.displayed_month;
    let first_weekday = first_day.weekday().num_days_from_monday() as usize;
    let dim = days_in_month(first_day.year(), first_day.month());

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            phase_label,
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "  ◄  {:>9} {:4}  ►",
                month_name(first_day.month()),
                first_day.year()
            ),
            Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Lu   Ma   Me   Je   Ve   Sa   Di",
            Style::default().fg(C_TEXT_DIM),
        )),
        Line::from(""),
    ];

    let mut day = 1u32;
    for week in 0..6 {
        let mut spans = vec![Span::raw("  ")];
        for col in 0..7usize {
            let cell_idx = week * 7 + col;
            if cell_idx < first_weekday || day > dim {
                spans.push(Span::raw("     "));
            } else {
                let date = NaiveDate::from_ymd_opt(first_day.year(), first_day.month(), day)
                    .unwrap_or(first_day);
                let is_cursor = date == cal.cursor;
                let is_start = cal.date_start == Some(date);
                let is_end = cal.date_end == Some(date);
                let in_range = match (cal.date_start, cal.date_end) {
                    (Some(s), Some(e)) => date > s && date < e,
                    _ => false,
                };

                let style = if is_cursor {
                    Style::default()
                        .fg(C_BG)
                        .bg(title_color)
                        .add_modifier(Modifier::BOLD)
                } else if is_start || is_end {
                    Style::default()
                        .fg(C_BG)
                        .bg(C_GREEN)
                        .add_modifier(Modifier::BOLD)
                } else if in_range {
                    Style::default().fg(C_TEXT).bg(C_SEL_BG)
                } else {
                    Style::default().fg(C_TEXT)
                };
                spans.push(Span::styled(format!("{:2}   ", day), style));
                day += 1;
            }
        }
        lines.push(Line::from(spans));
        if day > dim {
            break;
        }
    }

    lines.push(Line::from(""));
    let sel_text = match (cal.date_start, cal.date_end) {
        (Some(s), Some(e)) => format!("  {} → {}", s.format("%d/%m/%Y"), e.format("%d/%m/%Y")),
        (Some(s), None) => format!("  Début : {}  |  fin : ?", s.format("%d/%m/%Y")),
        _ => "  Aucune sélection".to_string(),
    };
    lines.push(Line::from(Span::styled(
        sel_text,
        Style::default().fg(C_ACCENT2),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ←/→ mois  ⇧←/→ année  ↑↓ sem.  Entrée ok  c effacer",
        Style::default().fg(C_TEXT_FAINT),
    )));

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(Span::styled(
                    " 📅 Filtre Temporel ",
                    Style::default()
                        .fg(title_color)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(title_color))
                .style(Style::default().bg(C_SURFACE2)),
        ),
        popup_area,
    );
}

// ─── Overlay modal ────────────────────────────────────────────────────────────

fn draw_overlay(f: &mut Frame, app: &App, area: Rect) {
    // ArtifactDetail
    if let OverlayState::ArtifactDetail {
        name,
        status,
        error,
        configs,
    } = &app.overlay
    {
        let popup_h = (14 + configs.len() as u16).min(area.height);
        let popup_area = Rect {
            x: area.x + area.width.saturating_sub(80) / 2,
            y: area.y + area.height.saturating_sub(popup_h) / 2,
            width: 80.min(area.width),
            height: popup_h,
        };
        f.render_widget(Clear, popup_area);

        let color = match status.as_str() {
            "STARTED" => C_GREEN,
            "ERROR" => C_RED,
            _ => C_AMBER,
        };

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(" Statut      : ", Style::default().fg(C_TEXT_DIM)),
                Span::styled(
                    status.clone(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                " Propriétés (Configurations) :",
                Style::default().fg(C_ACCENT),
            )),
        ];

        if configs.is_empty() {
            lines.push(Line::from(Span::styled(
                "   (Aucun paramètre externalisé)",
                Style::default().fg(C_TEXT_FAINT),
            )));
        } else {
            for (key, val) in configs {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("   ● {:<25}: ", key),
                        Style::default().fg(C_TEXT_DIM),
                    ),
                    Span::styled(val.clone(), Style::default().fg(C_TEXT)),
                ]));
            }
        }

        lines.extend([
            Line::from(""),
            Line::from(Span::styled(" Erreur :", Style::default().fg(C_TEXT_DIM))),
            Line::from(Span::styled(
                format!(
                    " {}",
                    error.as_deref().unwrap_or("Aucune erreur enregistrée.")
                ),
                Style::default().fg(C_RED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " Esc / Entrée pour fermer",
                Style::default().fg(C_TEXT_FAINT),
            )),
        ]);

        f.render_widget(
            Paragraph::new(lines)
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
                .wrap(Wrap { trim: false }),
            popup_area,
        );
        return;
    }

    // LogDetail
    if let OverlayState::LogDetail {
        guid,
        status,
        date,
        flow,
        error,
    } = &app.overlay
    {
        let popup_h = 18u16;
        let popup_w = 90u16;
        let popup_area = Rect {
            x: area.x + area.width.saturating_sub(popup_w) / 2,
            y: area.y + area.height.saturating_sub(popup_h) / 2,
            width: popup_w.min(area.width),
            height: popup_h.min(area.height),
        };
        f.render_widget(Clear, popup_area);

        let color = match status.as_str() {
            "COMPLETED" => C_GREEN,
            "FAILED" => C_RED,
            _ => C_AMBER,
        };

        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(" Statut : ", Style::default().fg(C_TEXT_DIM)),
                    Span::styled(
                        status.clone(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(" Date   : ", Style::default().fg(C_TEXT_DIM)),
                    Span::styled(date.clone(), Style::default().fg(C_TEXT)),
                ]),
                Line::from(vec![
                    Span::styled(" Flow   : ", Style::default().fg(C_TEXT_DIM)),
                    Span::styled(flow.clone(), Style::default().fg(C_BLUE)),
                ]),
                Line::from(vec![
                    Span::styled(" GUID   : ", Style::default().fg(C_TEXT_DIM)),
                    Span::styled(guid.clone(), Style::default().fg(C_ACCENT)),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    " Message d'erreur complet :",
                    Style::default().fg(C_TEXT_DIM),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!(" {}", error),
                    Style::default().fg(C_RED),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " Esc / Entrée pour fermer",
                    Style::default().fg(C_TEXT_FAINT),
                )),
            ])
            .block(
                Block::default()
                    .title(Span::styled(
                        " Détail du log ",
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Double)
                    .border_style(Style::default().fg(color))
                    .style(Style::default().bg(C_SURFACE2)),
            )
            .wrap(Wrap { trim: false }),
            popup_area,
        );
        return;
    }

    // Done / Error
    let popup_area = Rect {
        x: area.x + area.width.saturating_sub(52) / 2,
        y: area.y + area.height.saturating_sub(7) / 2,
        width: 52.min(area.width),
        height: 7.min(area.height),
    };
    f.render_widget(Clear, popup_area);

    let (title, body, color) = match &app.overlay {
        OverlayState::Done { message } => (" Terminé ", format!(" ●  {}", message), C_GREEN),
        OverlayState::Error { message } => (" Erreur ", format!(" ●  {}", message), C_RED),
        _ => return,
    };

    f.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                body,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " Appuyez sur Entrée",
                Style::default().fg(C_TEXT_FAINT),
            )),
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
        ),
        popup_area,
    );
}
