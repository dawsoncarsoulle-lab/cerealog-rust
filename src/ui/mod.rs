pub mod alerts;
pub mod artifacts;
pub mod configurations;
pub mod errors;
pub mod logs;
pub mod overview;
pub mod packages;
pub mod tenants;

use chrono::NaiveDateTime;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

use crate::app::{App, Tab};

pub fn render(frame: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(frame.size());

    render_header(frame, app, root[0]);
    render_tabs(frame, app, root[1]);

    match app.active_tab {
        Tab::Overview => overview::render(frame, app, root[2]),
        Tab::Logs => logs::render(frame, app, root[2]),
        Tab::Packages => packages::render(frame, app, root[2]),
        Tab::Artifacts => artifacts::render(frame, app, root[2]),
        Tab::Errors => errors::render(frame, app, root[2]),
        Tab::Configurations => configurations::render(frame, app, root[2]),
        Tab::Alerts => alerts::render(frame, app, root[2]),
    }

    render_footer(frame, app, root[3]);
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let tenant = app.selected_tenant.as_deref().unwrap_or("global");
    let query = app.active_query();
    let search = if query.is_empty() {
        "Search: -".to_string()
    } else {
        format!(
            "Search: \"{}\" | {}/{} resultats",
            query,
            app.current_match_count(),
            app.current_total_count()
        )
    };
    let refreshed = app
        .last_refresh
        .map(|instant| format!("{}s", instant.elapsed().as_secs()))
        .unwrap_or_else(|| "-".to_string());

    let line = Line::from(vec![
        Span::styled(
            "cerealog-tui",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  tenant: {tenant}  limit: {}  {search}  refresh: {refreshed}",
            app.limit
        )),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles = Tab::ALL.iter().map(|tab| Line::from(tab.title()));
    let selected = Tab::ALL
        .iter()
        .position(|tab| *tab == app.active_tab)
        .unwrap_or(0);
    let tabs = Tabs::new(titles)
        .select(selected)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider(" ");
    frame.render_widget(tabs, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let tenant_hint = if app.initial_tenant.is_none() {
        " t tenant  g global "
    } else {
        ""
    };
    let search_hint = if app.search_active() || app.editing_search {
        " Enter: detail  Esc: effacer "
    } else {
        " Enter: detail "
    };
    let text = format!(
        " Tab/Shift+Tab onglets  Up/Down navigation  / recherche {search_hint} r reload {} q quitter | {}",
        tenant_hint, app.status
    );
    frame.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

pub fn empty_message(message: &str) -> Paragraph<'_> {
    Paragraph::new(message.to_string())
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL))
}

pub fn selected_style() -> Style {
    Style::default()
        .bg(Color::Gray)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

pub fn matched_row_style() -> Style {
    Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

pub fn status_style(status: &str) -> Style {
    match status.to_ascii_uppercase().as_str() {
        "FAILED" => Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD),
        "COMPLETED" => Style::default().fg(Color::LightGreen),
        _ => Style::default(),
    }
}

pub fn highlight_match(text: &str, query: &str) -> Line<'static> {
    let terms = highlight_terms(query);
    if text.is_empty() || terms.is_empty() || !text.is_ascii() {
        return Line::from(text.to_string());
    }

    let lower = text.to_lowercase();
    let mut ranges = Vec::new();
    for term in terms {
        let mut start = 0;
        while let Some(pos) = lower[start..].find(&term) {
            let from = start + pos;
            let to = from + term.len();
            ranges.push((from, to));
            start = to;
        }
    }

    if ranges.is_empty() {
        return Line::from(text.to_string());
    }

    ranges.sort_unstable_by_key(|(start, _)| *start);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some((_, last_end)) = merged.last_mut() {
            if start <= *last_end {
                *last_end = (*last_end).max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    let mut spans = Vec::new();
    let mut cursor = 0;
    for (start, end) in merged {
        if cursor < start {
            spans.push(Span::raw(text[cursor..start].to_string()));
        }
        spans.push(Span::styled(
            text[start..end].to_string(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        cursor = end;
    }
    if cursor < text.len() {
        spans.push(Span::raw(text[cursor..].to_string()));
    }

    Line::from(spans)
}

fn highlight_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter_map(|raw| {
            let value = raw
                .split_once(':')
                .map(|(_, value)| value)
                .unwrap_or(raw)
                .trim()
                .to_lowercase();
            (!value.is_empty() && value.is_ascii()).then_some(value)
        })
        .collect()
}

pub fn fmt_dt(value: Option<NaiveDateTime>) -> String {
    value
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".to_string())
}

pub fn short(value: Option<&str>, max: usize) -> String {
    let value = value.unwrap_or("-");
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut result = value
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>();
    result.push_str("...");
    result
}
