use crate::ui::app::App;
use crate::ui::tabs::logs::{build_log_row, log_detail_text, render_scrollbar};
use crate::ui::theme::*;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

pub fn draw_deploy_errors_table(f: &mut Frame, app: &mut App, area: Rect) {
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

    let header = header_row(&["Artifact ID", "Tenant", "Date", "Message d'erreur"]);
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
                Cell::from(highlight_text(
                    err.tenant_id.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_BLUE),
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

pub fn draw_exec_errors_table(f: &mut Frame, app: &mut App, area: Rect) {
    let history_mode = app.exec_errors_history_mode;
    let (display_errors, total_count) = if history_mode {
        (app.filtered_exec_errors.as_slice(), app.exec_errors.len())
    } else {
        (
            app.filtered_active_exec_errors.as_slice(),
            app.active_exec_errors.len(),
        )
    };

    let mode_label = if history_mode {
        "Err.Exéc. — Historique complet  [h] actives"
    } else {
        "Err.Exéc. — Flows encore en FAILED  [h] historique"
    };

    if display_errors.is_empty() {
        let empty_msg = if history_mode {
            "  ● Aucune erreur d'exécution dans l'historique"
        } else {
            "  ● Aucune erreur active — tous les flows sont opérationnels"
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(empty_msg, Style::default().fg(C_GREEN))),
            ])
            .block(table_block(mode_label, 0, 0, "", false)),
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
        "Tenant",
        "Date",
        "Heure",
        "Aperçu erreur",
    ]);
    let query = app.search_query.clone();
    let has_date = app.date_filter.is_some();
    let rows: Vec<Row> = display_errors
        .iter()
        .map(|l| build_log_row(l, &query))
        .collect();
    let filtered_count = display_errors.len();
    let selected = app.selected().unwrap_or(0);

    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(38),
            Constraint::Length(24),
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Min(0),
        ],
    )
    .header(header)
    .block(table_block(
        mode_label,
        filtered_count,
        total_count,
        &query,
        has_date,
    ))
    .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ")
    .column_spacing(1);

    f.render_stateful_widget(table, chunks[0], &mut app.table_states[4]);
    render_scrollbar(f, chunks[0], filtered_count, selected);

    let selected_log = app.selected().and_then(|i| display_errors.get(i));
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
