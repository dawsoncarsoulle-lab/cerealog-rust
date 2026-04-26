use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Row},
};

// ─── Palette ─────────────────────────────────────────────────────────────────

pub const C_BG: Color = Color::Rgb(13, 13, 20);
pub const C_SURFACE: Color = Color::Rgb(22, 22, 32);
pub const C_SURFACE2: Color = Color::Rgb(30, 30, 44);
pub const C_BORDER: Color = Color::Rgb(50, 50, 70);
pub const C_BORDER_ACTIVE: Color = Color::Rgb(120, 100, 220);
pub const C_TEXT: Color = Color::Rgb(220, 220, 230);
pub const C_TEXT_DIM: Color = Color::Rgb(110, 110, 140);
pub const C_TEXT_FAINT: Color = Color::Rgb(60, 60, 80);
pub const C_ACCENT: Color = Color::Rgb(140, 110, 255);
pub const C_ACCENT2: Color = Color::Rgb(80, 200, 160);
pub const C_GREEN: Color = Color::Rgb(80, 200, 130);
pub const C_RED: Color = Color::Rgb(230, 80, 80);
pub const C_AMBER: Color = Color::Rgb(240, 170, 50);
pub const C_BLUE: Color = Color::Rgb(80, 160, 240);
pub const C_SEL_BG: Color = Color::Rgb(50, 40, 90);

// ─── Helpers ──────────────────────────────────────────────────────────────────

pub fn status_style(status: &str) -> Style {
    match status {
        "COMPLETED" => Style::default().fg(C_GREEN).add_modifier(Modifier::BOLD),
        "FAILED" | "ERROR" => Style::default().fg(C_RED).add_modifier(Modifier::BOLD),
        "STARTING" | "PROCESSING" => Style::default().fg(C_AMBER),
        "STARTED" => Style::default().fg(C_BLUE),
        _ => Style::default().fg(C_TEXT_DIM),
    }
}

pub fn status_icon(status: &str) -> &'static str {
    match status {
        "COMPLETED" | "STARTED" => "●",
        "FAILED" | "ERROR" => "●",
        "STARTING" | "PROCESSING" => "◌",
        _ => "○",
    }
}

pub fn header_row(cells: &[&str]) -> Row<'static> {
    Row::new(
        cells
            .iter()
            .map(|c| {
                Cell::from(c.to_string()).style(
                    Style::default()
                        .fg(C_TEXT_DIM)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                )
            })
            .collect::<Vec<_>>(),
    )
    .height(1)
    .style(Style::default().bg(C_SURFACE2))
}

pub fn table_block(
    title: &str,
    filtered: usize,
    total: usize,
    search: &str,
    has_date: bool,
) -> Block<'static> {
    let count = if search.is_empty() && !has_date {
        format!("({}) ", total)
    } else {
        format!("({}/{}) ", filtered, total)
    };
    Block::default()
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", title),
                Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(count, Style::default().fg(C_TEXT_DIM)),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(C_BORDER_ACTIVE))
        .style(Style::default().bg(C_SURFACE))
}

pub fn highlight_text(text: &str, query: &str, base_style: Style) -> Line<'static> {
    let hl = Style::default()
        .fg(C_BG)
        .bg(C_GREEN)
        .add_modifier(Modifier::BOLD);

    if query.is_empty() {
        return Line::from(Span::styled(text.to_string(), base_style));
    }

    let lt = text.to_lowercase();
    let lq = query.to_lowercase();
    let mut spans = Vec::new();
    let mut last = 0;

    for (start, part) in lt.match_indices(&lq) {
        if start > last {
            spans.push(Span::styled(text[last..start].to_string(), base_style));
        }
        spans.push(Span::styled(
            text[start..start + part.len()].to_string(),
            hl,
        ));
        last = start + part.len();
    }
    if last < text.len() {
        spans.push(Span::styled(text[last..].to_string(), base_style));
    }
    Line::from(spans)
}
