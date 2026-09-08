//! The full-screen popups: keyboard shortcuts, saved-conversation history,
//! and the code-block browser. Opening, navigating and acting within each
//! overlay lives here; the rendering side is in `crate::ui::overlay`.

use crate::app::{App, Cell};

/// Full-screen popups rendered on top of the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Shortcuts,
    History { selected: usize },
    Code { selected: usize },
    Models { selected: usize },
    Providers { selected: usize },
}

impl App {
    pub fn toggle_shortcuts(&mut self) {
        self.overlay = match self.overlay {
            Some(Overlay::Shortcuts) => None,
            _ => Some(Overlay::Shortcuts),
        };
    }

    pub fn toggle_history(&mut self) {
        if self.overlay.is_some_and(|o| matches!(o, Overlay::History { .. })) {
            self.overlay = None;
        } else {
            self.open_history();
        }
    }

    pub fn open_history(&mut self) {
        self.overlay = Some(Overlay::History {
            selected: self.sessions.current_index(),
        });
    }

    pub fn move_history_selection(&mut self, delta: i32) {
        let len = self.sessions.len();
        if len == 0 {
            return;
        }
        if let Some(Overlay::History { selected }) = &mut self.overlay {
            let next = (*selected as i32 + delta).rem_euclid(len as i32);
            *selected = next as usize;
        }
    }

    pub fn load_selected_session(&mut self) {
        let Some(Overlay::History { selected }) = self.overlay else {
            return;
        };
        if selected != self.sessions.current_index() {
            self.sessions.select(selected);
            self.rebuild_cells();
            self.scroll_from_bottom = 0;
        }
        self.overlay = None;
    }

    pub fn delete_selected_session(&mut self) {
        let Some(Overlay::History { selected }) = self.overlay else {
            return;
        };
        if self.sessions.len() <= 1 {
            return;
        }
        let was_current = selected == self.sessions.current_index();
        self.sessions.delete_at(selected);
        let len = self.sessions.len();
        if let Some(Overlay::History { selected }) = &mut self.overlay {
            *selected = (*selected).min(len - 1);
        }
        if was_current {
            self.rebuild_cells();
            self.scroll_from_bottom = 0;
        }
    }

    // -- code blocks -----------------------------------------------------------

    /// Every fenced code block in the conversation, in order. An in-flight
    /// streamed response is included too, so code can be copied while it is
    /// still being generated.
    pub fn code_blocks(&self) -> Vec<crate::code::CodeBlock> {
        let mut blocks = Vec::new();
        for cell in &self.cells {
            if let Cell::Assistant(text) = cell {
                blocks.extend(crate::code::extract(text));
            }
        }
        if !self.response.is_empty() {
            blocks.extend(crate::code::extract(&self.response));
        }
        blocks
    }

    pub fn open_code(&mut self) {
        if self.code_blocks().is_empty() {
            self.push_error("no code blocks in this conversation yet".into());
            return;
        }
        self.overlay = Some(Overlay::Code { selected: 0 });
    }

    pub fn toggle_code(&mut self) {
        if self.overlay.is_some_and(|o| matches!(o, Overlay::Code { .. })) {
            self.overlay = None;
        } else {
            self.open_code();
        }
    }

    pub fn move_code_selection(&mut self, delta: i32) {
        let len = self.code_blocks().len();
        if len == 0 {
            return;
        }
        if let Some(Overlay::Code { selected }) = &mut self.overlay {
            *selected = (*selected as i32 + delta).rem_euclid(len as i32) as usize;
        }
    }

    /// `Enter` in the code overlay: copy the selected block to the clipboard.
    pub fn copy_selected_code(&mut self) {
        let Some(Overlay::Code { selected }) = self.overlay else {
            return;
        };
        let blocks = self.code_blocks();
        let Some(block) = blocks.get(selected) else {
            self.overlay = None;
            return;
        };
        let lang = if block.lang.is_empty() {
            "code".to_string()
        } else {
            block.lang.clone()
        };
        let line_count = block.code.lines().count();
        match crate::clipboard::copy(&block.code) {
            Ok(outcome) if outcome.verified => self.push_notice(format!(
                "copied {lang} block ({line_count} lines) via {}",
                outcome.method
            )),
            Ok(outcome) => {
                // Best-effort OSC 52 path: the terminal may have ignored it,
                // so say "sent" instead of "copied" and point at the fix.
                let mut message = format!(
                    "sent {lang} block ({line_count} lines) via {} — paste to confirm",
                    outcome.method
                );
                if let Some(hint) = outcome.hint {
                    message.push_str(&format!(" ({hint})"));
                }
                self.push_notice(message);
            }
            Err(error) => self.push_error(format!("clipboard failed: {error}")),
        }
        self.overlay = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use crate::session::manager::SessionManager;

    fn test_app() -> App {
        App::new(Config::default(), SessionManager::for_tests())
    }

    #[test]
    fn code_blocks_are_collected_from_assistant_messages() {
        let mut app = test_app();
        app.cells.push(Cell::Assistant(
            "intro\n```rust\nlet x = 1;\n```\nmiddle\n```js\nlet y = 2;\n```".into(),
        ));
        app.response = "```py\nprint(1)".into(); // streamed, still unclosed
        let blocks = app.code_blocks();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].lang, "rust");
        assert_eq!(blocks[0].code, "let x = 1;");
        assert_eq!(blocks[1].lang, "js");
        assert_eq!(blocks[2].lang, "py");
    }

    #[test]
    fn code_overlay_opens_only_with_blocks() {
        let mut app = test_app();
        app.open_code();
        assert!(app.overlay.is_none());
        assert!(matches!(app.cells.last(), Some(Cell::Error(_))));
        app.cells.push(Cell::Assistant("```\ncode here\n```".into()));
        app.open_code();
        assert!(matches!(app.overlay, Some(Overlay::Code { selected: 0 })));
        app.move_code_selection(3); // wraps around with a single block
        assert!(matches!(app.overlay, Some(Overlay::Code { selected: 0 })));
    }

    #[test]
    fn history_overlay_moves_and_clamps() {
        let mut app = test_app();
        app.sessions.new_session();
        app.sessions.new_session();
        app.open_history();
        app.move_history_selection(5);
        if let Some(Overlay::History { selected }) = app.overlay {
            assert!(selected < app.sessions.len());
        } else {
            panic!("history overlay should be open");
        }
    }
}
