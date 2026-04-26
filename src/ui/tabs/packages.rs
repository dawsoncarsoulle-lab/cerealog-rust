use crate::db::ArtifactView;
use crate::ui::app::{App, PackageFocus};
use crate::ui::tabs::logs::{build_log_row, render_scrollbar};
use crate::ui::theme::*;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Row, Table},
    Frame,
};

pub fn draw_packages_master_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let header = header_row(&["ID", "Nom", "Version", "Tags", "Vendor", "Tenant"]);
    let query = app.search_query.clone();
    let has_date = app.date_filter.is_some();

    let pkg_rows: Vec<Row> = app
        .filtered_packages
        .iter()
        .map(|pkg| {
            Row::new(vec![
                Cell::from(highlight_text(
                    pkg.id.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_ACCENT),
                )),
                Cell::from(highlight_text(
                    pkg.name.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_TEXT),
                )),
                Cell::from(highlight_text(
                    pkg.version.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_TEXT_DIM),
                )),
                Cell::from(highlight_text(
                    pkg.tags.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_AMBER),
                )),
                Cell::from(highlight_text(
                    pkg.vendor.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_TEXT_FAINT),
                )),
                Cell::from(highlight_text(
                    pkg.tenant_id.as_deref().unwrap_or("—"),
                    &query,
                    Style::default().fg(C_BLUE),
                )),
            ])
            .height(1)
        })
        .collect();

    let total = app.packages.len();
    let filtered = app.filtered_packages.len();
    let list_border = if app.package_focus == PackageFocus::List {
        C_BORDER_ACTIVE
    } else {
        C_BORDER
    };
    let selected_pkg_idx = app.selected().unwrap_or(0);

    let pkg_table = Table::new(
        pkg_rows,
        [
            Constraint::Length(30),
            Constraint::Min(0),
            Constraint::Length(10),
            Constraint::Length(35),
            Constraint::Length(15),
            Constraint::Length(14),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(Line::from(vec![
                Span::styled(
                    " Integration Packages ",
                    Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    if query.is_empty() && !has_date {
                        format!("({}) ", total)
                    } else {
                        format!("({}/{}) ", filtered, total)
                    },
                    Style::default().fg(C_TEXT_DIM),
                ),
            ]))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(list_border))
            .style(Style::default().bg(C_SURFACE)),
    )
    .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ");

    f.render_stateful_widget(pkg_table, chunks[0], &mut app.table_states[2]);
    render_scrollbar(f, chunks[0], filtered, selected_pkg_idx);

    // ── Panneaux de détail
    let detail_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[1]);

    let pkg_id = app
        .selected()
        .and_then(|i| app.filtered_packages.get(i))
        .and_then(|p| p.id.as_deref())
        .unwrap_or("")
        .to_string();

    let pkg_artifacts: Vec<ArtifactView> = app
        .artifacts
        .iter()
        .filter(|a| a.package_id.as_deref() == Some(pkg_id.as_str()))
        .cloned()
        .collect();

    let valid_ids: std::collections::HashSet<String> = pkg_artifacts
        .iter()
        .flat_map(|a| {
            let mut v = vec![];
            if let Some(id) = &a.id {
                v.push(id.clone());
            }
            if let Some(n) = &a.name {
                v.push(n.clone());
            }
            v
        })
        .collect();

    let pkg_logs: Vec<_> = app
        .logs
        .iter()
        .filter(|l| {
            l.integration_flow_name
                .as_ref()
                .map(|f| valid_ids.contains(f))
                .unwrap_or(false)
        })
        .take(50)
        .cloned()
        .collect();

    let art_border = if app.package_focus == PackageFocus::Artifacts {
        C_ACCENT
    } else {
        C_BORDER
    };
    let log_border = if app.package_focus == PackageFocus::Activity {
        C_ACCENT
    } else {
        C_BORDER
    };

    let focus_hint = match app.package_focus {
        PackageFocus::List => " [Entrée] → naviguer dans les panneaux ",
        PackageFocus::Artifacts => " focus · [↑↓] naviguer · [Entrée] → Activité ",
        PackageFocus::Activity => " focus · [↑↓] naviguer · [Entrée] → Liste  · [Esc] ",
    };

    let art_rows: Vec<Row> = pkg_artifacts
        .iter()
        .map(|art| {
            let status = art.status.as_deref().unwrap_or("—");
            Row::new(vec![
                Cell::from(format!("  {} {}", status_icon(status), status))
                    .style(status_style(status)),
                Cell::from(art.name.as_deref().unwrap_or("Inconnu").to_string())
                    .style(Style::default().fg(C_TEXT)),
            ])
        })
        .collect();

    let mut art_state = app.pkg_art_state.clone();
    f.render_stateful_widget(
        Table::new(art_rows, [Constraint::Length(15), Constraint::Min(0)])
            .header(header_row(&["  Statut", "Artifact"]))
            .block(
                Block::default()
                    .title(Span::styled(
                        format!(" Artifacts inclus ({}) ", pkg_artifacts.len()),
                        Style::default().fg(C_ACCENT2).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(art_border))
                    .style(Style::default().bg(C_SURFACE2)),
            )
            .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ "),
        detail_chunks[0],
        &mut art_state,
    );
    app.pkg_art_state = art_state;

    let log_rows: Vec<Row> = pkg_logs
        .iter()
        .map(|log| {
            let status = log.status.as_deref().unwrap_or("—");
            Row::new(vec![
                Cell::from(format!("  {} {}", status_icon(status), status))
                    .style(status_style(status)),
                Cell::from(
                    log.parsed_date
                        .map(|d| d.format("%d/%m %H:%M").to_string())
                        .unwrap_or_default(),
                )
                .style(Style::default().fg(C_TEXT_DIM)),
                Cell::from(
                    log.integration_flow_name
                        .as_deref()
                        .unwrap_or("—")
                        .chars()
                        .take(20)
                        .collect::<String>(),
                )
                .style(Style::default().fg(C_BLUE)),
                Cell::from(
                    log.error_message
                        .as_deref()
                        .unwrap_or("")
                        .chars()
                        .take(40)
                        .collect::<String>(),
                )
                .style(Style::default().fg(C_TEXT_FAINT)),
            ])
        })
        .collect();

    let mut log_state = app.pkg_log_state.clone();
    f.render_stateful_widget(
        Table::new(
            log_rows,
            [
                Constraint::Length(15),
                Constraint::Length(14),
                Constraint::Length(22),
                Constraint::Min(0),
            ],
        )
        .header(header_row(&[
            "  Statut",
            "Date",
            "Source",
            "Erreur / Détail",
        ]))
        .block(
            Block::default()
                .title(Line::from(vec![
                    Span::styled(
                        format!(" Activité récente ({}) ", pkg_logs.len()),
                        Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(focus_hint, Style::default().fg(C_TEXT_FAINT)),
                ]))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(log_border))
                .style(Style::default().bg(C_SURFACE2)),
        )
        .highlight_style(Style::default().bg(C_SEL_BG).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ "),
        detail_chunks[1],
        &mut log_state,
    );
    app.pkg_log_state = log_state;
}
