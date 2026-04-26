use crate::ui::app::App;
use crate::ui::theme::*;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, BorderType, Borders, Gauge, Paragraph, Sparkline},
    Frame,
};

pub fn draw_stats_and_charts(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let stat_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(cols[0]);
    let top_stats = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(stat_rows[0]);
    let bot_stats = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(stat_rows[1]);

    let cards = [
        (
            "TOTAL LOGS",
            app.stats.total_logs.to_string(),
            C_BLUE,
            "▲",
            top_stats[0],
        ),
        (
            "ERREURS",
            app.stats.failed_logs.to_string(),
            if app.stats.failed_logs > 0 {
                C_RED
            } else {
                C_GREEN
            },
            "!",
            top_stats[1],
        ),
        (
            "PACKAGES",
            app.stats.total_packages.to_string(),
            C_ACCENT,
            "◈",
            bot_stats[0],
        ),
        (
            "ARTIFACTS",
            app.stats.total_artifacts.to_string(),
            C_ACCENT2,
            "◉",
            bot_stats[1],
        ),
    ];

    for (label, value, color, icon, rect) in cards {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(format!(" {} ", icon), Style::default().fg(color)),
                    Span::styled(
                        value,
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(Span::styled(
                    format!("  {}", label),
                    Style::default().fg(C_TEXT_FAINT),
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(C_BORDER))
                    .style(Style::default().bg(C_SURFACE)),
            ),
            rect,
        );
    }

    let chart_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(cols[1]);

    let sparkline_data = App::padded_sparkline(&app.error_sparkline, 24);
    f.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .title(" Erreurs / heure (24h) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(C_BORDER))
                    .border_type(BorderType::Rounded),
            )
            .data(&sparkline_data)
            .style(Style::default().fg(C_RED)),
        chart_rows[0],
    );

    let bars: Vec<Bar> = app
        .error_barchart
        .iter()
        .map(|(label, val)| Bar::default().value(*val).label(label.as_str().into()))
        .collect();
    f.render_widget(
        BarChart::default()
            .block(
                Block::default()
                    .title(" Erreurs / jour (7j) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(C_BORDER))
                    .border_type(BorderType::Rounded),
            )
            .data(BarGroup::default().bars(&bars))
            .bar_width(5)
            .bar_gap(1)
            .bar_style(Style::default().fg(C_AMBER))
            .value_style(Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)),
        chart_rows[1],
    );
}

pub fn draw_analytics(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(rows[0]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    let total = app.stats.total_logs.max(1) as f64;
    let completed = (app.stats.total_logs - app.stats.failed_logs).max(0) as f64;
    let ratio = (completed / total).clamp(0.0, 1.0);
    let pct = (ratio * 100.0) as u16;
    let gauge_color = if pct >= 90 {
        C_GREEN
    } else if pct >= 70 {
        C_AMBER
    } else {
        C_RED
    };

    f.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .title(" Taux de Succès ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(C_BORDER_ACTIVE))
                    .style(Style::default().bg(C_SURFACE)),
            )
            .gauge_style(Style::default().fg(gauge_color).bg(C_SURFACE2))
            .ratio(ratio)
            .label(format!(
                "{}%  ({} / {})",
                pct, completed as i64, app.stats.total_logs
            )),
        top[0],
    );

    let bars: Vec<Bar> = app
        .top_errors_barchart
        .iter()
        .map(|(label, val)| {
            Bar::default()
                .value(*val)
                .label(label.chars().take(12).collect::<String>().into())
                .style(Style::default().fg(C_RED))
        })
        .collect();
    f.render_widget(
        BarChart::default()
            .block(
                Block::default()
                    .title(" Top 5 Artifacts en Erreur ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(C_BORDER))
                    .style(Style::default().bg(C_SURFACE)),
            )
            .data(BarGroup::default().bars(&bars))
            .bar_width(6)
            .bar_gap(1)
            .value_style(Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD)),
        top[1],
    );

    let activity_data = App::padded_sparkline(&app.activity_sparkline, 12);
    f.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .title(" Volume d'activité global / heure (12h) ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(C_BORDER))
                    .style(Style::default().bg(C_SURFACE)),
            )
            .data(&activity_data)
            .style(Style::default().fg(C_BLUE)),
        bottom[0],
    );

    let status_entries = [
        ("COMPLETED", "●", C_GREEN),
        ("FAILED", "●", C_RED),
        ("PROCESSING", "◌", C_AMBER),
        ("STARTED", "●", C_BLUE),
        ("ERROR", "●", C_RED),
    ];
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " Répartition des statuts",
            Style::default().fg(C_TEXT_DIM).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for (status, icon, color) in status_entries {
        let count = app
            .status_counts
            .iter()
            .find(|(s, _)| s == status)
            .map(|(_, c)| *c)
            .unwrap_or(0);
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(color)),
            Span::styled(format!("{:<12}", status), Style::default().fg(C_TEXT)),
            Span::styled(
                format!("{}", count),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(""));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(" Statuts ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(C_BORDER))
                .style(Style::default().bg(C_SURFACE)),
        ),
        bottom[1],
    );
}
