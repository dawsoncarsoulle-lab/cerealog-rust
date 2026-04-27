pub mod app;
pub mod footer;
pub mod overlay;
pub mod tabs;
pub mod theme;

pub use app::{App, OverlayState};

use crate::models::RefreshData;
use crate::ui::app::{CalendarState, PackageFocus};
use crate::ui::footer::{draw_footer, draw_header};
use crate::ui::overlay::{draw_overlay, draw_overlay_calendar, draw_overlay_tenant};
use crate::ui::tabs::{
    analytics::{draw_analytics, draw_stats_and_charts},
    artifacts::draw_artifacts_table,
    errors::{draw_deploy_errors_table, draw_exec_errors_table},
    logs::draw_logs_master_detail,
    packages::draw_packages_master_detail,
    Tab,
};
use crate::ui::theme::C_BG;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::Style,
    widgets::Block,
    Frame, Terminal,
};
use std::io;
use std::time::{Duration, Instant};

// ─── Événements retournés à main ─────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum AppEvent {
    Quit,
    TriggerRefresh { full: bool },
    LoadMore,
    Continue,
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

                if let OverlayState::TenantFilter { selected } = app.overlay {
                    let mut new_selected = selected;
                    let tenants = app.available_tenants();
                    let total = tenants.len() + 1;

                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            app.overlay = OverlayState::Hidden;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            new_selected = new_selected.saturating_sub(1);
                            app.overlay = OverlayState::TenantFilter {
                                selected: new_selected,
                            };
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            new_selected = (new_selected + 1).min(total - 1);
                            app.overlay = OverlayState::TenantFilter {
                                selected: new_selected,
                            };
                        }
                        KeyCode::Enter => {
                            if new_selected == 0 {
                                app.tenant_filter = None;
                            } else {
                                app.tenant_filter = Some(tenants[new_selected - 1].clone());
                            }
                            app.overlay = OverlayState::Hidden;
                            app.apply_filters();
                        }
                        _ => {}
                    }
                    continue;
                }
                // ── Overlay Calendrier
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

                // ── Overlays génériques
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

                // ── Mode recherche
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

                // ── Raccourcis globaux
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
                    KeyCode::Char('h') if app.active_tab == Tab::ExecErrors => {
                        app.exec_errors_history_mode = !app.exec_errors_history_mode;
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
                    KeyCode::Char('t') => {
                        let selected = app
                            .available_tenants()
                            .iter()
                            .position(|t| Some(t) == app.tenant_filter.as_ref())
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        app.overlay = OverlayState::TenantFilter { selected };
                    }

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
                        } else if app.exec_errors_history_mode {
                            app.selected().and_then(|i| app.filtered_exec_errors.get(i))
                        } else {
                            app.selected()
                                .and_then(|i| app.filtered_active_exec_errors.get(i))
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
        OverlayState::TenantFilter { .. } => draw_overlay_tenant(f, app, size),
        _ => draw_overlay(f, app, size),
    }
}

fn draw_body(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    match app.active_tab {
        Tab::Logs => draw_logs_master_detail(f, app, area),
        Tab::Artifacts => draw_artifacts_table(f, app, area),
        Tab::Packages => draw_packages_master_detail(f, app, area),
        Tab::DeployErrors => draw_deploy_errors_table(f, app, area),
        Tab::ExecErrors => draw_exec_errors_table(f, app, area),
        Tab::Analytics => draw_analytics(f, app, area),
    }
}
