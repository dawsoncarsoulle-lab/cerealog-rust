use crate::ui::app::{App, OverlayState};
use crate::ui::tabs::Tab;
use crate::ui::theme::*;
use chrono::Datelike;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Tabs},
    Frame,
};

pub fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(36), Constraint::Min(0)])
        .split(area);

    let age = app.last_refresh.elapsed().as_secs();
    let freshness = if age < 60 {
        format!("{}s", age)
    } else {
        format!("{}m{}s", age / 60, age % 60)
    };
    let freshness_color = if age > 300 { C_RED } else { C_TEXT_FAINT };

    const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let spinner_str = if app.refreshing {
        format!(
            "{} ",
            SPINNER[(app.refresh_spinner as usize) % SPINNER.len()]
        )
    } else {
        String::new()
    };

    let cal_indicator = if let Some((s, e)) = app.date_filter {
        format!("📅 {}→{}  ", s.format("%d/%m"), e.format("%d/%m"))
    } else {
        String::new()
    };

    let title_line = Line::from(vec![
        Span::styled(
            "SAP",
            Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " BTP",
            Style::default().fg(C_ACCENT2).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · Monitor  ", Style::default().fg(C_TEXT_DIM)),
        Span::styled(
            format!("↻ {}  ", freshness),
            Style::default().fg(freshness_color),
        ),
        Span::styled(spinner_str, Style::default().fg(C_ACCENT)),
        Span::styled(cal_indicator, Style::default().fg(C_BLUE)),
    ]);

    let title = Paragraph::new(title_line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(C_BORDER))
                .style(Style::default().bg(C_SURFACE)),
        )
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(title, chunks[0]);

    let tab_labels: Vec<Line> = Tab::all()
        .iter()
        .map(|&t| {
            let line = tab_label(t, app);
            if t == app.active_tab {
                line.patch_style(Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD))
            } else {
                line.patch_style(Style::default().fg(C_TEXT_DIM))
            }
        })
        .collect();

    let tabs = Tabs::new(tab_labels)
        .select(app.active_tab.index())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(C_BORDER))
                .style(Style::default().bg(C_SURFACE)),
        )
        .highlight_style(
            Style::default()
                .fg(C_ACCENT)
                .bg(C_SURFACE2)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled(" │ ", Style::default().fg(C_BORDER)));

    f.render_widget(tabs, chunks[1]);
}

fn tab_label(tab: Tab, app: &App) -> Line<'static> {
    let (name, counts, is_alert) = match tab {
        Tab::Logs => (
            "Logs",
            Some((app.filtered_logs.len(), app.logs.len())),
            false,
        ),
        Tab::Artifacts => (
            "Artifacts",
            Some((app.filtered_artifacts.len(), app.artifacts.len())),
            false,
        ),
        Tab::Packages => (
            "Packages",
            Some((app.filtered_packages.len(), app.packages.len())),
            false,
        ),
        Tab::DeployErrors => (
            "Err.Déploi.",
            Some((app.filtered_deploy_errors.len(), app.deploy_errors.len())),
            !app.filtered_deploy_errors.is_empty(),
        ),
        Tab::ExecErrors => (
            "Err.Exéc.",
            Some((app.filtered_exec_errors.len(), app.exec_errors.len())),
            !app.filtered_exec_errors.is_empty(),
        ),
        Tab::Analytics => ("Analytics", None, false),
    };

    let badge_color = if is_alert { C_RED } else { C_TEXT_FAINT };
    let mut spans = vec![Span::raw(format!("  {} ", name))];
    if let Some((filtered, total)) = counts {
        let has_filter = !app.search_query.is_empty() || app.date_filter.is_some();
        let badge = if has_filter {
            format!("[{}/{}]  ", filtered, total)
        } else {
            format!("[{}]  ", total)
        };
        spans.push(Span::styled(badge, Style::default().fg(badge_color)));
    } else {
        spans.push(Span::raw("  "));
    }
    Line::from(spans)
}

pub fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    if app.search_active {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    " 🔍 Recherche : ",
                    Style::default()
                        .fg(C_BG)
                        .bg(C_GREEN)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {} ", app.search_query),
                    Style::default().fg(C_TEXT),
                ),
                Span::styled("█", Style::default().fg(C_ACCENT)),
            ]))
            .style(Style::default().bg(C_SURFACE2)),
            area,
        );
        return;
    }

    let mut spans: Vec<Span> = vec![Span::raw("  ")];
    let shortcuts = [
        ("↑↓/jk", "nav"),
        ("Tab", "onglet"),
        ("Enter", "détail"),
        ("type", "chercher"),
        ("Esc", "effacer"),
        ("d", "calendrier"),
        ("D", "effacer date"),
        ("+", "500 logs"),
        ("r", "refresh-logs"),
        ("q", "quitter"),
        ("R", "full-refresh"),
        ("t", "tenant"),
    ];
    for (key, action) in shortcuts {
        spans.push(Span::styled(
            format!(" {} ", key),
            Style::default().fg(C_BG).bg(C_ACCENT),
        ));
        spans.push(Span::styled(
            format!(" {}  ", action),
            Style::default().fg(C_TEXT_DIM),
        ));
    }
    if app.date_filter.is_some() {
        spans.push(Span::styled(
            " 📅 DATE ACTIVE ",
            Style::default()
                .fg(C_BG)
                .bg(C_BLUE)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(ref t) = app.tenant_filter {
        spans.push(Span::styled(
            format!(" ◉ {} ", t),
            Style::default()
                .fg(C_BG)
                .bg(C_AMBER)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if app.refreshing {
        spans.push(Span::styled(
            " ⟳ REFRESH… ",
            Style::default()
                .fg(C_BG)
                .bg(C_ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(C_SURFACE)),
        area,
    );
}
