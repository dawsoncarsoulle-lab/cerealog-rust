use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

use crate::app::App;

use super::{
    empty_message, fmt_dt, highlight_match, matched_row_style, selected_style, short, status_style,
};

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
    let indices = app.filtered_artifact_indices();
    if indices.is_empty() {
        let message = if app.search_active() {
            format!("Aucun resultat pour \"{}\".", app.active_query())
        } else {
            "Aucun artifact a afficher.".to_string()
        };
        frame.render_widget(empty_message(&message), area);
        return;
    }

    let rows = indices.iter().map(|index| {
        let artifact = &app.data.artifacts[*index];
        let status = artifact.status.clone().unwrap_or_else(|| "-".to_string());
        let row = Row::new(vec![
            Cell::from(artifact.tenant_id.clone()),
            Cell::from(highlight_match(
                &short(Some(&artifact.id), 28),
                app.active_query(),
            )),
            Cell::from(highlight_match(
                &short(artifact.name.as_deref(), 32),
                app.active_query(),
            )),
            Cell::from(highlight_match(&status, app.active_query())).style(status_style(&status)),
            Cell::from(highlight_match(
                &short(artifact.package_id.as_deref(), 28),
                app.active_query(),
            )),
            Cell::from(fmt_dt(artifact.deployed_on)),
            Cell::from(highlight_match(
                &short(artifact.artifact_type.as_deref(), 16),
                app.active_query(),
            )),
        ]);
        if app.search_active() {
            row.style(matched_row_style())
        } else {
            row
        }
    });

    let mut state = TableState::default();
    state.select(Some(app.selections.artifacts.min(indices.len() - 1)));

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Percentage(22),
            Constraint::Percentage(26),
            Constraint::Length(12),
            Constraint::Percentage(18),
            Constraint::Length(19),
            Constraint::Length(16),
        ],
    )
    .header(
        Row::new([
            "tenant_id",
            "id",
            "name",
            "status",
            "package_id",
            "deployed_on",
            "artifact_type",
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().title("Artifacts").borders(Borders::ALL))
    .highlight_style(selected_style());

    frame.render_stateful_widget(table, area, &mut state);
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let Some(artifact) = app.selected_artifact() else {
        frame.render_widget(empty_message("Selection vide."), area);
        return;
    };
    let errors = app
        .data
        .errors
        .iter()
        .filter(|error| error.tenant_id == artifact.tenant_id && error.artifact_id == artifact.id)
        .take(4)
        .map(|error| {
            format!(
                "- {} | {}",
                fmt_dt(error.error_time),
                error.error_message.as_deref().unwrap_or("-")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let configs = app
        .data
        .configurations
        .iter()
        .filter(|config| {
            config.tenant_id == artifact.tenant_id && config.artifact_id == artifact.id
        })
        .take(6)
        .map(|config| {
            format!(
                "- {} = {}",
                config.parameter_key,
                config.parameter_value.as_deref().unwrap_or("-")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let text = format!(
        "tenant_id: {}\nid: {}\nname: {}\nstatus: {}\npackage_id: {}\ndeployed_on: {}\nartifact_type: {}\n\nErreurs:\n{}\n\nConfigurations:\n{}",
        artifact.tenant_id,
        artifact.id,
        artifact.name.as_deref().unwrap_or("-"),
        artifact.status.as_deref().unwrap_or("-"),
        artifact.package_id.as_deref().unwrap_or("-"),
        fmt_dt(artifact.deployed_on),
        artifact.artifact_type.as_deref().unwrap_or("-"),
        if errors.is_empty() { "-" } else { &errors },
        if configs.is_empty() { "-" } else { &configs }
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().title("Detail").borders(Borders::ALL)),
        area,
    );
}
