use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

use crate::app::App;

use super::{empty_message, highlight_match, matched_row_style, selected_style, short};

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
    let indices = app.filtered_configuration_indices();
    if indices.is_empty() {
        let message = if app.search_active() {
            format!("Aucun resultat pour \"{}\".", app.active_query())
        } else {
            "Aucune configuration a afficher.".to_string()
        };
        frame.render_widget(empty_message(&message), area);
        return;
    }

    let rows = indices.iter().map(|index| {
        let config = &app.data.configurations[*index];
        let row = Row::new(vec![
            Cell::from(config.tenant_id.clone()),
            Cell::from(highlight_match(
                &short(Some(&config.artifact_id), 30),
                app.active_query(),
            )),
            Cell::from(highlight_match(
                &short(Some(&config.parameter_key), 32),
                app.active_query(),
            )),
            Cell::from(highlight_match(
                &short(config.parameter_value.as_deref(), 44),
                app.active_query(),
            )),
            Cell::from(config.data_type.clone().unwrap_or_else(|| "-".to_string())),
        ]);
        if app.search_active() {
            row.style(matched_row_style())
        } else {
            row
        }
    });

    let mut state = TableState::default();
    state.select(Some(app.selections.configurations.min(indices.len() - 1)));

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Percentage(24),
            Constraint::Percentage(26),
            Constraint::Percentage(36),
            Constraint::Length(14),
        ],
    )
    .header(
        Row::new([
            "tenant_id",
            "artifact_id",
            "parameter_key",
            "parameter_value",
            "data_type",
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .title("Configurations")
            .borders(Borders::ALL),
    )
    .highlight_style(selected_style());

    frame.render_stateful_widget(table, area, &mut state);
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let Some(config) = app.selected_configuration() else {
        frame.render_widget(empty_message("Selection vide."), area);
        return;
    };
    let text = format!(
        "tenant_id: {}\nartifact_id: {}\nparameter_key: {}\nparameter_value: {}\ndata_type: {}",
        config.tenant_id,
        config.artifact_id,
        config.parameter_key,
        config.parameter_value.as_deref().unwrap_or("-"),
        config.data_type.as_deref().unwrap_or("-")
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().title("Detail").borders(Borders::ALL)),
        area,
    );
}
