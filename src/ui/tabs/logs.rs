use crate::db::LogView;
use crate::ui::app::App;
use crate::ui::theme::*;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{
        Block, BorderType, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, Wrap,
    },
    Frame,
};

pub fn draw_logs_master_detail(f: &mut Frame, app: &mut App, area: Rect) {
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
    let rows: Vec<Row> = app
        .filtered_logs
        .iter()
        .map(|l| build_log_row(l, &query))
        .collect();
    let total = app.logs.len();
    let filtered = app.filtered_logs.len();
    let title = if query.is_empty() {
        "Logs d'exécution".to_string()
    } else {
        format!("Logs · '{}'", query)
    };
    let block = table_block(&title, filtered, total, &query, has_date);
    let selected = app.selected().unwrap_or(0);

    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(38),
            Constraint::Length(24),
            Constraint::Length(14), // Tenant
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Min(0),
        ],
    )
    .header(header)
    .block(block)
    .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ")
    .column_spacing(1);

    f.render_stateful_widget(table, chunks[0], &mut app.table_states[0]);
    render_scrollbar(f, chunks[0], filtered, selected);

    let selected_log = app.selected().and_then(|i| app.filtered_logs.get(i));
    let border_color = log_border_color(selected_log);
    let detail = Paragraph::new(log_detail_text(selected_log))
        .block(
            Block::default()
                .title(Span::styled(
                    " Vue Détaillée — [Entrée] pour tout afficher ",
                    Style::default()
                        .fg(border_color)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color))
                .style(Style::default().bg(C_SURFACE)),
        )
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(C_TEXT));
    f.render_widget(detail, chunks[1]);
}

pub fn build_log_row(log: &LogView, query: &str) -> Row<'static> {
    let status = log.status.as_deref().unwrap_or("—");
    let (date_str, time_str) = log
        .parsed_date
        .map(|d| {
            (
                d.format("%d/%m/%Y").to_string(),
                d.format("%H:%M:%S").to_string(),
            )
        })
        .unwrap_or(("—".into(), "—".into()));
    let guid = log.message_guid.as_deref().unwrap_or("—");
    let flow = log
        .integration_flow_name
        .as_deref()
        .unwrap_or("—")
        .chars()
        .take(22)
        .collect::<String>();
    let tenant = log
        .tenant_id
        .as_deref()
        .unwrap_or("—")
        .chars()
        .take(12)
        .collect::<String>();
    let err = log
        .error_message
        .as_deref()
        .unwrap_or("")
        .chars()
        .take(50)
        .collect::<String>();

    Row::new(vec![
        Cell::from(highlight_text(
            &format!("  {} {}", status_icon(status), status),
            query,
            status_style(status),
        )),
        Cell::from(highlight_text(guid, query, Style::default().fg(C_ACCENT))),
        Cell::from(highlight_text(&flow, query, Style::default().fg(C_BLUE))),
        Cell::from(highlight_text(&tenant, query, Style::default().fg(C_AMBER))),
        Cell::from(highlight_text(
            &date_str,
            query,
            Style::default().fg(C_TEXT_DIM),
        )),
        Cell::from(highlight_text(
            &time_str,
            query,
            Style::default().fg(C_TEXT_DIM),
        )),
        Cell::from(highlight_text(
            &err,
            query,
            Style::default().fg(C_TEXT_FAINT),
        )),
    ])
    .height(1)
}

pub fn log_detail_text(log: Option<&LogView>) -> String {
    log.map(|l| {
        let date = l.parsed_date.map(|d| d.format("%d/%m/%Y %H:%M:%S").to_string()).unwrap_or_default();
        format!(
            "GUID    : {}\nDate    : {}\nStatut  : {}\nTenant  : {}\nFlow    : {}\n\nErreur  :\n{}\n\n[Entrée] pour afficher l'erreur complète",
            l.message_guid.as_deref().unwrap_or("—"),
            date,
            l.status.as_deref().unwrap_or("—"),
            l.tenant_id.as_deref().unwrap_or("—"),
            l.integration_flow_name.as_deref().unwrap_or("—"),
            l.error_message.as_deref().unwrap_or("Aucune information d'erreur."),
        )
    })
    .unwrap_or_else(|| "Sélectionnez une ligne · [Entrée] pour le détail complet".to_string())
}

pub fn log_border_color(log: Option<&LogView>) -> ratatui::style::Color {
    log.and_then(|l| l.status.as_deref())
        .map(|s| match s {
            "FAILED" => C_RED,
            "COMPLETED" => C_GREEN,
            _ => C_BORDER,
        })
        .unwrap_or(C_BORDER)
}

pub fn render_scrollbar(f: &mut Frame, area: Rect, len: usize, selected: usize) {
    let mut state = ScrollbarState::default()
        .content_length(len)
        .position(selected);
    f.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓")),
        area.inner(&ratatui::layout::Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut state,
    );
}
