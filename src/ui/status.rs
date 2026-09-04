//! The `• Working (12s • esc to interrupt)` row rendered above the composer
//! while a response streams, matching codex's status indicator.

use crate::app::App;
use crate::ui::theme::{self, SPINNER};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let glyph = SPINNER[(app.elapsed_ms() / 120) as usize % SPINNER.len()];
    let line = Line::from(vec![
        Span::styled(glyph, Style::new().fg(theme::ACCENT).bold()),
        Span::raw(" "),
        Span::styled("Working", Style::new().bold()),
        Span::raw(" "),
        Span::styled(
            format!("({} • ", theme::fmt_elapsed(app.elapsed_secs())),
            theme::dim(),
        ),
        Span::raw("esc"),
        Span::styled(" to interrupt)", theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
