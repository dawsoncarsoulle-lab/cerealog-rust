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
    let indices = app.filtered_error_indices();
    if indices.is_empty() {
        let message = if app.search_active() {
            format!("Aucun resultat pour \"{}\".", app.active_query())
        } else {
            "Aucune erreur artifact a afficher.".to_string()
        };
        frame.render_widget(empty_message(&message), area);
        return;
    }

    let rows = indices.iter().map(|index| {
        let error = &app.data.errors[*index];
        let row = Row::new(vec![
            Cell::from(error.tenant_id.clone()),
            Cell::from(highlight_match(
                &short(Some(&error.artifact_id), 34),
                app.active_query(),
            )),
            Cell::from(fmt_dt(error.error_time)),
            Cell::from(highlight_match(
                &short(error.error_message.as_deref(), 70),
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
    state.select(Some(app.selections.errors.min(indices.len() - 1)));

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Percentage(25),
            Constraint::Length(19),
            Constraint::Percentage(55),
        ],
    )
    .header(
        Row::new(["tenant_id", "artifact_id", "error_time", "error_message"]).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().title("Errors").borders(Borders::ALL))
    .highlight_style(selected_style());

    frame.render_stateful_widget(table, area, &mut state);
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let Some(error) = app.selected_error() else {
        frame.render_widget(empty_message("Selection vide."), area);
        return;
    };
    let text = format!(
        "tenant_id: {}\nartifact_id: {}\nerror_time: {}\n\n{}",
        error.tenant_id,
        error.artifact_id,
        fmt_dt(error.error_time),
        error.error_message.as_deref().unwrap_or("-")
    );
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().title("Detail").borders(Borders::ALL)),
        area,
    );
}
