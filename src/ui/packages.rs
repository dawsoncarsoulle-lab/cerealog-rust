use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

use crate::app::App;

use super::{empty_message, fmt_dt, highlight_match, matched_row_style, selected_style, short};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let constraints = if app.detail_open {
        [Constraint::Percentage(65), Constraint::Percentage(35)]
    } else {
        [Constraint::Percentage(100), Constraint::Length(0)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    render_table(frame, app, chunks[0]);
    if app.detail_open {
        render_detail(frame, app, chunks[1]);
    }
}

fn render_table(frame: &mut Frame, app: &App, area: Rect) {
    let indices = app.filtered_package_indices();
    if indices.is_empty() {
        let message = if app.search_active() {
            format!("Aucun resultat pour \"{}\".", app.active_query())
        } else {
            "Aucun package a afficher.".to_string()
        };
        frame.render_widget(empty_message(&message), area);
        return;
    }

    let rows = indices.iter().map(|index| {
        let package = &app.data.packages[*index];
        let row = Row::new(vec![
            Cell::from(package.tenant_id.clone()),
            Cell::from(highlight_match(
                &short(Some(&package.id), 28),
                app.active_query(),
            )),
            Cell::from(highlight_match(
                &short(package.name.as_deref(), 34),
                app.active_query(),
            )),
            Cell::from(highlight_match(
                package.version.as_deref().unwrap_or("-"),
                app.active_query(),
            )),
            Cell::from(highlight_match(
                &short(package.vendor.as_deref(), 24),
                app.active_query(),
            )),
            Cell::from(fmt_dt(package.creation_date)),
            Cell::from(short(package.tags.as_deref(), 28)),
            Cell::from(package.artifact_count.to_string()),
        ]);
        if app.search_active() {
            row.style(matched_row_style())
        } else {
            row
        }
    });

    let mut state = TableState::default();
    state.select(Some(app.selections.packages.min(indices.len() - 1)));

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Percentage(18),
            Constraint::Percentage(20),
            Constraint::Length(12),
            Constraint::Percentage(14),
            Constraint::Length(19),
            Constraint::Percentage(18),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new([
            "tenant_id",
            "id",
            "name",
            "version",
            "vendor",
            "creation_date",
            "tags",
            "artifacts",
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().title("Packages").borders(Borders::ALL))
    .highlight_style(selected_style());

    frame.render_stateful_widget(table, area, &mut state);
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let Some(package) = app.selected_package() else {
        frame.render_widget(empty_message("Selection vide."), area);
        return;
    };
    let artifacts = app
        .data
        .artifacts
        .iter()
        .filter(|artifact| {
            artifact.tenant_id == package.tenant_id
                && artifact.package_id.as_deref() == Some(package.id.as_str())
        })
        .take(8)
        .map(|artifact| {
            format!(
                "- {} | {} | {}",
                artifact.id,
                artifact.name.as_deref().unwrap_or("-"),
                artifact.status.as_deref().unwrap_or("-")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let text = format!(
        "tenant_id: {}\nid: {}\nname: {}\nversion: {}\nvendor: {}\ncreation_date: {}\ntags: {}\n\nArtifacts lies:\n{}",
        package.tenant_id,
        package.id,
        package.name.as_deref().unwrap_or("-"),
        package.version.as_deref().unwrap_or("-"),
        package.vendor.as_deref().unwrap_or("-"),
        fmt_dt(package.creation_date),
        package.tags.as_deref().unwrap_or("-"),
        if artifacts.is_empty() { "-" } else { &artifacts }
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().title("Detail").borders(Borders::ALL)),
        area,
    );
}
