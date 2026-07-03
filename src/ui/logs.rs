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
    let indices = app.filtered_log_indices();
    if indices.is_empty() {
        let message = if app.search_active() {
            format!("Aucun resultat pour \"{}\".", app.active_query())
        } else {
            "Aucun log a afficher.".to_string()
        };
        frame.render_widget(empty_message(&message), area);
        return;
    }

    let rows = indices.iter().map(|index| {
        let log = &app.data.logs[*index];
        let status = log.status.clone().unwrap_or_else(|| "-".to_string());
        let row = Row::new(vec![
            Cell::from(log.tenant_id.clone()),
            Cell::from(fmt_dt(log.parsed_date)),
            Cell::from(highlight_match(&status, app.active_query())).style(status_style(&status)),
            Cell::from(highlight_match(
                &short(log.integration_flow_name.as_deref(), 28),
                app.active_query(),
            )),
            Cell::from(highlight_match(
                &short(Some(&log.message_guid), 28),
                app.active_query(),
            )),
            Cell::from(highlight_match(
                &short(log.error_message.as_deref(), 40),
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
    state.select(Some(app.selections.logs.min(indices.len() - 1)));

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(19),
            Constraint::Length(12),
            Constraint::Percentage(24),
            Constraint::Percentage(22),
            Constraint::Percentage(30),
        ],
    )
    .header(
        Row::new([
            "tenant_id",
            "parsed_date",
            "status",
            "integration_flow_name",
            "message_guid",
            "error_message",
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().title("Logs").borders(Borders::ALL))
    .highlight_style(selected_style());

    frame.render_stateful_widget(table, area, &mut state);
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let Some(log) = app.selected_log() else {
        frame.render_widget(empty_message("Selection vide."), area);
        return;
    };

    let text = format!(
        "tenant_id: {}\nparsed_date: {}\nstatus: {}\nintegration_flow_name: {}\nmessage_guid: {}\n\n{}",
        log.tenant_id,
        fmt_dt(log.parsed_date),
        log.status.as_deref().unwrap_or("-"),
        log.integration_flow_name.as_deref().unwrap_or("-"),
        log.message_guid,
        log.error_message.as_deref().unwrap_or("Pas de message d'erreur.")
    );

    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().title("Detail").borders(Borders::ALL)),
        area,
    );
}
