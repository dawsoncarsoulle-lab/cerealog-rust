use crate::ui::app::App;
use crate::ui::tabs::logs::render_scrollbar;
use crate::ui::theme::*;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table},
    Frame,
};

pub fn draw_artifacts_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header = header_row(&["  Statut", "ID", "Nom de l'artifact", "Package", "Tenant"]);
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
                Cell::from(highlight_text(
                    art.tenant_id.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_AMBER),
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
            Constraint::Length(14),
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
