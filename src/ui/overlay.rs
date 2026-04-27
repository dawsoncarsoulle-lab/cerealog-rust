use crate::ui::app::{App, CalendarState, OverlayState};
use crate::ui::theme::*;
use chrono::{Datelike, NaiveDate};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

pub fn draw_overlay(f: &mut Frame, app: &App, area: Rect) {
    if let OverlayState::ArtifactDetail {
        name,
        status,
        error,
        configs,
    } = &app.overlay
    {
        let popup_h = (14 + configs.len() as u16).min(area.height);
        let popup_area = Rect {
            x: area.x + area.width.saturating_sub(80) / 2,
            y: area.y + area.height.saturating_sub(popup_h) / 2,
            width: 80.min(area.width),
            height: popup_h,
        };
        f.render_widget(Clear, popup_area);

        let color = match status.as_str() {
            "STARTED" => C_GREEN,
            "ERROR" => C_RED,
            _ => C_AMBER,
        };

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(" Statut      : ", Style::default().fg(C_TEXT_DIM)),
                Span::styled(
                    status.clone(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                " Propriétés (Configurations) :",
                Style::default().fg(C_ACCENT),
            )),
        ];

        if configs.is_empty() {
            lines.push(Line::from(Span::styled(
                "   (Aucun paramètre externalisé)",
                Style::default().fg(C_TEXT_FAINT),
            )));
        } else {
            for (key, val) in configs {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("   ● {:<25}: ", key),
                        Style::default().fg(C_TEXT_DIM),
                    ),
                    Span::styled(val.clone(), Style::default().fg(C_TEXT)),
                ]));
            }
        }

        lines.extend([
            Line::from(""),
            Line::from(Span::styled(" Erreur :", Style::default().fg(C_TEXT_DIM))),
            Line::from(Span::styled(
                format!(
                    " {}",
                    error.as_deref().unwrap_or("Aucune erreur enregistrée.")
                ),
                Style::default().fg(C_RED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " Esc / Entrée pour fermer",
                Style::default().fg(C_TEXT_FAINT),
            )),
        ]);

        f.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .title(Span::styled(
                            format!(" {} ", name),
                            Style::default().fg(color).add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Double)
                        .border_style(Style::default().fg(color))
                        .style(Style::default().bg(C_SURFACE2)),
                )
                .wrap(Wrap { trim: false }),
            popup_area,
        );
        return;
    }

    if let OverlayState::LogDetail {
        guid,
        status,
        date,
        flow,
        error,
    } = &app.overlay
    {
        let popup_area = Rect {
            x: area.x + area.width.saturating_sub(90) / 2,
            y: area.y + area.height.saturating_sub(18) / 2,
            width: 90.min(area.width),
            height: 18.min(area.height),
        };
        f.render_widget(Clear, popup_area);

        let color = match status.as_str() {
            "COMPLETED" => C_GREEN,
            "FAILED" => C_RED,
            _ => C_AMBER,
        };

        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(" Statut : ", Style::default().fg(C_TEXT_DIM)),
                    Span::styled(
                        status.clone(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(" Date   : ", Style::default().fg(C_TEXT_DIM)),
                    Span::styled(date.clone(), Style::default().fg(C_TEXT)),
                ]),
                Line::from(vec![
                    Span::styled(" Flow   : ", Style::default().fg(C_TEXT_DIM)),
                    Span::styled(flow.clone(), Style::default().fg(C_BLUE)),
                ]),
                Line::from(vec![
                    Span::styled(" GUID   : ", Style::default().fg(C_TEXT_DIM)),
                    Span::styled(guid.clone(), Style::default().fg(C_ACCENT)),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    " Message d'erreur complet :",
                    Style::default().fg(C_TEXT_DIM),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!(" {}", error),
                    Style::default().fg(C_RED),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " Esc / Entrée pour fermer",
                    Style::default().fg(C_TEXT_FAINT),
                )),
            ])
            .block(
                Block::default()
                    .title(Span::styled(
                        " Détail du log ",
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Double)
                    .border_style(Style::default().fg(color))
                    .style(Style::default().bg(C_SURFACE2)),
            )
            .wrap(Wrap { trim: false }),
            popup_area,
        );
        return;
    }

    // Done / Error
    let popup_area = Rect {
        x: area.x + area.width.saturating_sub(52) / 2,
        y: area.y + area.height.saturating_sub(7) / 2,
        width: 52.min(area.width),
        height: 7.min(area.height),
    };
    f.render_widget(Clear, popup_area);

    let (title, body, color) = match &app.overlay {
        OverlayState::Done { message } => (" Terminé ", format!(" ●  {}", message), C_GREEN),
        OverlayState::Error { message } => (" Erreur ", format!(" ●  {}", message), C_RED),
        _ => return,
    };

    f.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                body,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " Appuyez sur Entrée",
                Style::default().fg(C_TEXT_FAINT),
            )),
        ])
        .block(
            Block::default()
                .title(Span::styled(
                    title,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(color))
                .style(Style::default().bg(C_SURFACE2)),
        ),
        popup_area,
    );
}

pub fn draw_overlay_calendar(f: &mut Frame, app: &App, area: Rect) {
    let cal = if let OverlayState::Calendar(ref c) = app.overlay {
        c
    } else {
        return;
    };

    let popup_area = Rect {
        x: area.x + area.width.saturating_sub(48) / 2,
        y: area.y + area.height.saturating_sub(20) / 2,
        width: 48.min(area.width),
        height: 20.min(area.height),
    };
    f.render_widget(Clear, popup_area);

    let title_color = if cal.phase == 0 { C_BLUE } else { C_ACCENT };
    let phase_label = if cal.phase == 0 {
        "  ① Choisissez la date de DÉBUT"
    } else {
        "  ② Choisissez la date de FIN  "
    };

    let first_day = cal.displayed_month;
    let first_weekday = first_day.weekday().num_days_from_monday() as usize;
    let dim = days_in_month(first_day.year(), first_day.month());

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            phase_label,
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "  ◄  {:>9} {:4}  ►",
                month_name(first_day.month()),
                first_day.year()
            ),
            Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Lu   Ma   Me   Je   Ve   Sa   Di",
            Style::default().fg(C_TEXT_DIM),
        )),
        Line::from(""),
    ];

    let mut day = 1u32;
    for week in 0..6 {
        let mut spans = vec![Span::raw("  ")];
        for col in 0..7usize {
            let cell_idx = week * 7 + col;
            if cell_idx < first_weekday || day > dim {
                spans.push(Span::raw("     "));
            } else {
                let date = NaiveDate::from_ymd_opt(first_day.year(), first_day.month(), day)
                    .unwrap_or(first_day);
                let is_cursor = date == cal.cursor;
                let is_start = cal.date_start == Some(date);
                let is_end = cal.date_end == Some(date);
                let in_range = match (cal.date_start, cal.date_end) {
                    (Some(s), Some(e)) => date > s && date < e,
                    _ => false,
                };
                let style = if is_cursor {
                    Style::default()
                        .fg(C_BG)
                        .bg(title_color)
                        .add_modifier(Modifier::BOLD)
                } else if is_start || is_end {
                    Style::default()
                        .fg(C_BG)
                        .bg(C_GREEN)
                        .add_modifier(Modifier::BOLD)
                } else if in_range {
                    Style::default().fg(C_TEXT).bg(C_SEL_BG)
                } else {
                    Style::default().fg(C_TEXT)
                };
                spans.push(Span::styled(format!("{:2}   ", day), style));
                day += 1;
            }
        }
        lines.push(Line::from(spans));
        if day > dim {
            break;
        }
    }

    lines.push(Line::from(""));
    let sel_text = match (cal.date_start, cal.date_end) {
        (Some(s), Some(e)) => format!("  {} → {}", s.format("%d/%m/%Y"), e.format("%d/%m/%Y")),
        (Some(s), None) => format!("  Début : {}  |  fin : ?", s.format("%d/%m/%Y")),
        _ => "  Aucune sélection".to_string(),
    };
    lines.push(Line::from(Span::styled(
        sel_text,
        Style::default().fg(C_ACCENT2),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ←/→ mois  ⇧←/→ année  ↑↓ sem.  Entrée ok  c effacer",
        Style::default().fg(C_TEXT_FAINT),
    )));

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(Span::styled(
                    " 📅 Filtre Temporel ",
                    Style::default()
                        .fg(title_color)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(title_color))
                .style(Style::default().bg(C_SURFACE2)),
        ),
        popup_area,
    );
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(30)
}

fn month_name(m: u32) -> &'static str {
    match m {
        1 => "Janvier",
        2 => "Février",
        3 => "Mars",
        4 => "Avril",
        5 => "Mai",
        6 => "Juin",
        7 => "Juillet",
        8 => "Août",
        9 => "Septembre",
        10 => "Octobre",
        11 => "Novembre",
        _ => "Décembre",
    }
}

pub fn draw_overlay_tenant(f: &mut Frame, app: &App, area: Rect) {
    let tenants = app.available_tenants();
    let selected = if let OverlayState::TenantFilter { selected } = app.overlay {
        selected
    } else {
        return;
    };

    let total_items = tenants.len() + 1;
    let popup_h = (total_items as u16 + 5).min(area.height);
    let popup_w = 42u16;
    let popup_area = Rect {
        x: area.x + area.width.saturating_sub(popup_w) / 2,
        y: area.y + area.height.saturating_sub(popup_h) / 2,
        width: popup_w.min(area.width),
        height: popup_h,
    };
    f.render_widget(Clear, popup_area);

    let mut lines: Vec<Line> = vec![];

    let all_style = if selected == 0 {
        Style::default()
            .bg(C_SEL_BG)
            .fg(C_ACCENT2)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(C_ACCENT2)
    };
    lines.push(Line::from(vec![
        Span::styled(if selected == 0 { " ▶ " } else { "   " }, all_style),
        Span::styled("● ", Style::default().fg(C_ACCENT2)),
        Span::styled("Tous les tenants", all_style),
    ]));

    let tenant_colors = [C_ACCENT, C_AMBER, C_BLUE, C_RED, C_GREEN];
    for (i, tenant) in tenants.iter().enumerate() {
        let idx = i + 1;
        let color = tenant_colors[i % tenant_colors.len()];
        let is_sel = selected == idx;
        let style = if is_sel {
            Style::default()
                .bg(C_SEL_BG)
                .fg(color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(C_TEXT)
        };
        lines.push(Line::from(vec![
            Span::styled(if is_sel { " ▶ " } else { "   " }, style),
            Span::styled("● ", Style::default().fg(color)),
            Span::styled(tenant.clone(), style),
        ]));
    }

    let block = Block::default()
        .title(Span::styled(
            " Filtrer par tenant ",
            Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(C_ACCENT))
        .style(Style::default().bg(C_SURFACE2));

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    for (i, line) in lines.iter().enumerate() {
        if i >= inner.height as usize {
            break;
        }
        let row = Rect {
            x: inner.x,
            y: inner.y + i as u16,
            width: inner.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(line.clone()), row);
    }

    let footer_y = inner.y + inner.height.saturating_sub(1);
    f.render_widget(
        Paragraph::new(Span::styled(
            " ↑↓ naviguer · Entrée sélectionner · Esc fermer",
            Style::default().fg(C_TEXT_FAINT),
        )),
        Rect {
            x: inner.x,
            y: footer_y,
            width: inner.width,
            height: 1,
        },
    );
}
