//! The codex-style bottom pane: a borderless `› ` composer that grows with
//! content, the slash-command popup anchored above it, and the hint/context
//! footer row.

use crate::app::{App, MAX_COMPOSER_ROWS};
use crate::ui::theme;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

/// Number of wrapped rows the composer needs at `width`.
pub fn row_count(app: &App, width: u16) -> u16 {
    wrapped_rows(app, width.max(4) as usize).len() as u16
}

fn wrapped_rows(app: &App, width: usize) -> Vec<Line<'static>> {
    if app.composer.is_empty() {
        return vec![Line::from(vec![
            theme::user_prefix(),
            Span::styled(crate::app::PLACEHOLDER.to_string(), theme::dim()),
        ])];
    }
    // Support explicit newlines (shift/alt+enter): wrap each segment.
    let mut rows: Vec<Line<'static>> = Vec::new();
    for (index, segment) in app.composer.split('\n').enumerate() {
        let (first, cont) = if index == 0 {
            (theme::user_prefix(), Span::raw("  "))
        } else {
            (Span::raw("  "), Span::raw("  "))
        };
        rows.extend(theme::wrap_styled(
            vec![Span::raw(segment.to_string())],
            width,
            first,
            cont,
        ));
    }
    if rows.len() > MAX_COMPOSER_ROWS {
        // Keep the tail so the cursor stays visible, like codex's textarea.
        let keep = rows.split_off(rows.len() - MAX_COMPOSER_ROWS);
        let mut kept = keep;
        if let Some(first) = kept.first_mut() {
            if let Some(prefix) = first.spans.first_mut() {
                *prefix = Span::raw("  ");
            }
        }
        return kept;
    }
    rows
}

pub fn render_prompt(frame: &mut Frame, area: Rect, app: &App) -> Option<Position> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let rows = wrapped_rows(app, area.width as usize);
    let visible = rows.len().min(area.height as usize);
    let start = rows.len().saturating_sub(visible);
    for (index, line) in rows[start..].iter().enumerate() {
        frame.render_widget(
            Paragraph::new(line.clone()),
            Rect {
                x: area.x,
                y: area.y + index as u16,
                width: area.width,
                height: 1,
            },
        );
    }
    // The cursor always sits at the end of the text.
    let last = rows.last()?;
    let col = theme::line_width(last).min(area.width as usize - 1);
    Some(Position::new(
        area.x + col as u16,
        area.y + visible.saturating_sub(1) as u16,
    ))
}

pub fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let left: Vec<Span<'static>> = if app.quit_primed() {
        vec![Span::styled(
            "press ctrl+c again to quit",
            Style::new().fg(Color::Yellow),
        )]
    } else if app.streaming {
        Vec::new()
    } else if app.composer.is_empty() && app.overlay.is_none() {
        vec![Span::styled("? for shortcuts", theme::dim())]
    } else {
        Vec::new()
    };
    let right = vec![Span::styled(app.context_summary(), theme::dim())];
    let right_w = theme::spans_width(&right);
    let left_w = theme::spans_width(&left);
    let gap = 2;
    let left_fits = left_w + gap + right_w <= area.width as usize;

    if left_fits && !left.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(left)),
            Rect {
                width: area.width - right_w as u16 - gap as u16,
                ..area
            },
        );
    }
    if right_w < area.width as usize {
        frame.render_widget(
            Paragraph::new(Line::from(right)),
            Rect {
                x: area.right() - right_w as u16,
                width: right_w as u16,
                ..area
            },
        );
    }
}

/// Popup rows for the slash-command palette (already bordered), or empty.
pub fn slash_popup_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let items = app.slash_filtered();
    if items.is_empty() {
        return vec![];
    }
    let selected = app.slash_selected.min(items.len() - 1);
    let max_rows = 6usize;
    let start = selected.saturating_sub(max_rows - 1);
    let end = (start + max_rows).min(items.len());
    let shown = &items[start..end];

    let name_w = shown.iter().map(|c| c.name.width()).max().unwrap_or(0);
    let desc_limit = width.saturating_sub(name_w + 6).max(8);

    let mut rows: Vec<(String, bool)> = shown
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let desc: String = item.desc.chars().take(desc_limit).collect();
            (
                format!("{:<name_w$}  {}", item.name, desc),
                start + index == selected,
            )
        })
        .collect();
    // Equalize widths so the highlight bar spans the full popup.
    let row_w = rows
        .iter()
        .map(|(text, _)| text.width())
        .max()
        .unwrap_or(0)
        .min(width.saturating_sub(6));
    for (text, _) in &mut rows {
        let pad = row_w.saturating_sub(text.width());
        text.push_str(&" ".repeat(pad));
    }
    let lines: Vec<Line<'static>> = rows
        .into_iter()
        .map(|(text, is_selected)| {
            Line::from(vec![
                Span::raw("  "),
                if is_selected {
                    Span::styled(text, Style::new().bg(theme::SELECT_BG))
                } else {
                    Span::raw(text)
                },
            ])
        })
        .collect();
    theme::with_border(lines)
}
