//! Full-screen overlays: the keyboard-shortcuts popup (`?` / `/help`) and the
//! conversation-history picker (`ctrl+t` / `/history`). Both use codex's dim
//! rounded card style, centered over the interface.

use crate::app::{App, Overlay};
use crate::ui::theme;
use ratatui::prelude::*;
use ratatui::widgets::{Clear, Paragraph};

pub fn render(frame: &mut Frame, area: Rect, app: &App, overlay: Overlay) {
    match overlay {
        Overlay::Shortcuts => shortcuts(frame, area),
        Overlay::History { selected } => history(frame, area, app, selected),
        Overlay::Code { selected } => code(frame, area, app, selected),
    }
}

fn shortcuts(frame: &mut Frame, area: Rect) {
    let rows: [(&str, &str); 10] = [
        ("enter", "send message"),
        ("esc", "close popup · interrupt stream"),
        ("ctrl+t", "conversation history"),
        ("ctrl+g", "copy code blocks"),
        ("ctrl+r", "show / hide model reasoning"),
        ("pgup / pgdn", "scroll transcript"),
        ("up / down", "prompt history"),
        ("← / →", "move cursor · ctrl jumps words"),
        ("shift+enter", "newline in composer"),
        ("ctrl+c ×2", "quit"),
    ];
    let key_w = rows.iter().map(|(key, _)| key.len()).max().unwrap_or(0);

    let mut lines = vec![Line::from(Span::styled(
        "Keyboard shortcuts",
        Style::new().bold(),
    ))];
    lines.push(Line::from(""));
    for (key, desc) in rows {
        lines.push(Line::from(vec![
            Span::raw(format!("{key:<key_w$}  ")),
            Span::styled(desc, theme::dim()),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Slash commands",
        Style::new().bold(),
    )));
    lines.push(Line::from(""));
    for command in crate::app::SLASH_COMMANDS {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<key_w$}  ", command.name),
                Style::new().fg(theme::ACCENT),
            ),
            Span::styled(command.desc, theme::dim()),
        ]));
    }
    render_card(frame, area, lines);
}

fn history(frame: &mut Frame, area: Rect, app: &App, selected: usize) {
    let sessions = app.sessions.sessions();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Conversation history", Style::new().bold()),
            Span::styled(
                "   ↑↓ select · enter open · d delete · esc close",
                theme::dim(),
            ),
        ]),
        Line::from(""),
    ];
    if sessions.is_empty() {
        lines.push(Line::styled("no saved conversations", theme::dim()));
        render_card(frame, area, lines);
        return;
    }
    let max_rows = crate::app::OVERLAY_ROWS;
    let start = selected.saturating_sub(max_rows - 1);
    let end = (start + max_rows).min(sessions.len());
    for (index, session) in sessions[start..end].iter().enumerate() {
        let index = start + index;
        let is_selected = index == selected;
        let marker = if is_selected { "> " } else { "  " };
        let label = format!("{marker}{}", session.title);
        let meta = format!("  · {} msgs", session.messages.len());
        let line = if is_selected {
            Line::from(vec![
                Span::styled(label, Style::new().bold().bg(theme::SELECT_BG)),
                Span::styled(meta, theme::dim().bg(theme::SELECT_BG)),
            ])
        } else {
            Line::from(vec![Span::raw(label), Span::styled(meta, theme::dim())])
        };
        lines.push(line);
    }
    render_card(frame, area, lines);
}

fn code(frame: &mut Frame, area: Rect, app: &App, selected: usize) {
    let blocks = app.code_blocks();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Code blocks", Style::new().bold()),
            Span::styled("   ↑↓ select · enter copy · esc close", theme::dim()),
        ]),
        Line::from(""),
    ];
    if blocks.is_empty() {
        lines.push(Line::styled(
            "no code blocks in this conversation",
            theme::dim(),
        ));
        render_card(frame, area, lines);
        return;
    }
    let max_rows = crate::app::OVERLAY_ROWS;
    let start = selected.saturating_sub(max_rows - 1);
    let end = (start + max_rows).min(blocks.len());
    for (index, block) in blocks[start..end].iter().enumerate() {
        let index = start + index;
        let is_selected = index == selected;
        let marker = if is_selected { "> " } else { "  " };
        let lang = if block.lang.is_empty() {
            "code"
        } else {
            block.lang.as_str()
        };
        let line_count = block.code.lines().count();
        let preview: String = block
            .code
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(48)
            .collect();
        let label = format!(
            "{marker}{:>2} · {lang} · {line_count} lines · {preview}",
            index + 1
        );
        let line = if is_selected {
            Line::from(Span::styled(
                label,
                Style::new().bold().bg(theme::SELECT_BG),
            ))
        } else {
            Line::from(label)
        };
        lines.push(line);
    }
    render_card(frame, area, lines);
}

/// Center a dim rounded card around `lines` and render it.
fn render_card(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    let bordered = theme::with_border(lines);
    let width = bordered
        .iter()
        .map(theme::line_width)
        .max()
        .unwrap_or(0)
        .min(area.width as usize);
    let height = bordered.len().min(area.height as usize);
    if width == 0 || height == 0 {
        return;
    }
    let x = area.x + (area.width.saturating_sub(width as u16)) / 2;
    let y = area.y + (area.height.saturating_sub(height as u16)) / 2;
    let rect = Rect {
        x,
        y,
        width: width as u16,
        height: height as u16,
    };
    // Erase everything underneath the card first. Without this, content drawn
    // behind the popup — especially shaded code blocks in the transcript —
    // bleeds into the card and the two layers visually fight each other.
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(bordered), rect);
}
