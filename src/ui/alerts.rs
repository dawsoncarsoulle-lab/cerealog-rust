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
    let root_constraints = if app.detail_open {
        [Constraint::Percentage(65), Constraint::Percentage(35)]
    } else {
        [Constraint::Percentage(100), Constraint::Length(0)]
    };
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints(root_constraints)
        .split(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(root[0]);
    render_pending(frame, app, chunks[0]);
    render_smart(frame, app, chunks[1]);
    if app.detail_open {
        render_detail(frame, app, root[1]);
    }
}

fn render_pending(frame: &mut Frame, app: &App, area: Rect) {
    let all_indices = app.filtered_alert_indices();
    let pending_len = app.data.alerts.pending.len();
    let indices = all_indices
        .iter()
        .copied()
        .filter(|index| *index < pending_len)
        .collect::<Vec<_>>();
    if indices.is_empty() {
        let message = if app.search_active() {
            format!(
                "Aucun resultat pending_alerts pour \"{}\".",
                app.active_query()
            )
        } else {
            "Aucune pending_alert a afficher.".to_string()
        };
        frame.render_widget(empty_message(&message), area);
        return;
    }

    let rows = indices.iter().map(|index| {
        let alert = &app.data.alerts.pending[*index];
        let row = Row::new(vec![
            Cell::from(alert.tenant_id.clone()),
            Cell::from(short(Some(&alert.log_guid), 24)),
            Cell::from(highlight_match(
                &short(alert.flow_name.as_deref(), 28),
                app.active_query(),
            )),
            Cell::from(short(alert.error_type.as_deref(), 18)),
            Cell::from(fmt_dt(alert.detected_at)),
            Cell::from(highlight_match(
                &short(alert.error_snippet.as_deref(), 48),
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
    let selected_global = all_indices.get(app.selections.alerts).copied();
    state.select(
        selected_global.and_then(|global| indices.iter().position(|index| *index == global)),
    );

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Percentage(18),
            Constraint::Percentage(22),
            Constraint::Length(16),
            Constraint::Length(19),
            Constraint::Percentage(30),
        ],
    )
    .header(
        Row::new([
            "tenant_id",
            "log_guid",
            "flow_name",
            "error_type",
            "detected_at",
            "error_snippet",
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .title("pending_alerts")
            .borders(Borders::ALL),
    )
    .highlight_style(selected_style());

    frame.render_stateful_widget(table, area, &mut state);
}

fn render_smart(frame: &mut Frame, app: &App, area: Rect) {
    let all_indices = app.filtered_alert_indices();
    let pending_len = app.data.alerts.pending.len();
    let indices = all_indices
        .iter()
        .copied()
        .filter(|index| *index >= pending_len)
        .map(|index| index - pending_len)
        .collect::<Vec<_>>();
    if indices.is_empty() {
        let message = if app.search_active() {
            format!(
                "Aucun resultat smart_alerts pour \"{}\".",
                app.active_query()
            )
        } else {
            "Aucune smart_alert a afficher.".to_string()
        };
        frame.render_widget(empty_message(&message), area);
        return;
    }

    let rows = indices.iter().map(|index| {
        let alert = &app.data.alerts.smart[*index];
        let row = Row::new(vec![
            Cell::from(alert.tenant_id.clone()),
            Cell::from(highlight_match(
                &short(Some(&alert.flow_name), 34),
                app.active_query(),
            )),
            Cell::from(highlight_match(&alert.alert_type, app.active_query())),
            Cell::from(highlight_match(&alert.status, app.active_query()))
                .style(status_style(&alert.status)),
            Cell::from(fmt_dt(alert.last_triggered_at)),
            Cell::from(short(alert.extra.as_deref(), 40)),
        ]);
        if app.search_active() {
            row.style(matched_row_style())
        } else {
            row
        }
    });

    let mut state = TableState::default();
    let selected_global = all_indices.get(app.selections.alerts).copied();
    state.select(selected_global.and_then(|global| {
        global
            .checked_sub(pending_len)
            .and_then(|smart| indices.iter().position(|index| *index == smart))
    }));

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Percentage(40),
            Constraint::Length(16),
            Constraint::Length(14),
            Constraint::Length(19),
            Constraint::Percentage(26),
        ],
    )
    .header(
        Row::new([
            "tenant_id",
            "flow_name",
            "alert_type",
            "status",
            "last_triggered_at",
            "extra",
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().title("smart_alerts").borders(Borders::ALL))
    .highlight_style(selected_style());

    frame.render_stateful_widget(table, area, &mut state);
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let Some(text) = app.selected_alert_text() else {
        frame.render_widget(empty_message("Selection vide."), area);
        return;
    };
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().title("Detail").borders(Borders::ALL)),
        area,
    );
}
