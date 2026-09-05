use crate::api::client::ApiClient;
use crate::config::Config;
use crate::session::manager::SessionManager;
use anyhow::Result;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{self, Receiver};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PLACEHOLDER: &str = "Ask chatTUI to do anything";
pub const MAX_COMPOSER_ROWS: usize = 8;
/// Number of rows visible inside overlay lists (history / code); also the
/// page size for pgup/pgdn navigation within an overlay.
pub const OVERLAY_ROWS: usize = 12;
const QUIT_PRIME_WINDOW: Duration = std::time::Duration::from_secs(2);

pub struct SlashCmd {
    pub name: &'static str,
    pub desc: &'static str,
}

pub const SLASH_COMMANDS: &[SlashCmd] = &[
    SlashCmd { name: "/help", desc: "Show keyboard shortcuts" },
    SlashCmd { name: "/new", desc: "Start a new conversation" },
    SlashCmd { name: "/history", desc: "Browse saved conversations" },
    SlashCmd { name: "/code", desc: "Browse & copy code blocks" },
    SlashCmd { name: "/model", desc: "Switch model — /model <model-id>" },
    SlashCmd { name: "/quit", desc: "Exit chatTUI" },
];

/// A rendered entry of the conversation transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    User(String),
    Assistant(String),
    Error(String),
    Notice(String),
}

/// Full-screen popups rendered on top of the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Shortcuts,
    History { selected: usize },
    Code { selected: usize },
}

pub struct App {
    pub config: Config,
    pub sessions: SessionManager,
    pub cells: Vec<Cell>,
    pub response: String,
    pub streaming: bool,
    pub stream_started: Option<Instant>,
    pub tokens: Option<Receiver<Result<String>>>,
    pub composer: String,
    /// Byte offset of the editing cursor inside `composer` (char boundary).
    pub cursor: usize,
    pub prompt_history: Vec<String>,
    pub history_nav: Option<(usize, String)>,
    pub scroll_from_bottom: u16,
    pub overlay: Option<Overlay>,
    pub slash_selected: usize,
    pub quit_primed_at: Option<Instant>,
    pub should_quit: bool,
}

impl App {
    pub fn new(config: Config, sessions: SessionManager) -> Self {
        let mut app = Self {
            config,
            sessions,
            cells: Vec::new(),
            response: String::new(),
            streaming: false,
            stream_started: None,
            tokens: None,
            composer: String::new(),
            cursor: 0,
            prompt_history: Vec::new(),
            history_nav: None,
            scroll_from_bottom: 0,
            overlay: None,
            slash_selected: 0,
            quit_primed_at: None,
            should_quit: false,
        };
        app.rebuild_cells();
        app
    }

    fn rebuild_cells(&mut self) {
        self.cells = self
            .sessions
            .current()
            .messages
            .iter()
            .map(|message| match message.role.as_str() {
                "assistant" => Cell::Assistant(message.content.clone()),
                _ => Cell::User(message.content.clone()),
            })
            .collect();
    }

    // -- transient UI actions -------------------------------------------------

    /// `Esc`: close popups first, then interrupt a running stream, then clear the composer.
    pub fn escape(&mut self) {
        if self.overlay.take().is_some() {
            return;
        }
        if self.streaming {
            self.interrupt();
            return;
        }
        if !self.composer.is_empty() {
            self.clear_composer();
        }
    }

    pub fn interrupt(&mut self) {
        self.tokens = None;
        self.streaming = false;
        self.stream_started = None;
        self.finish_partial();
    }

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
            Ok(method) => self.push_notice(format!(
                "copied {lang} block ({line_count} lines) via {method}"
            )),
            Err(error) => self.push_error(format!("clipboard failed: {error}")),
        }
        self.overlay = None;
    }

    pub fn scroll(&mut self, delta: i32) {
        let next = self.scroll_from_bottom as i32 + delta;
        self.scroll_from_bottom = next.clamp(0, u16::MAX as i32) as u16;
    }

    // -- composer --------------------------------------------------------------

    pub fn on_composer_changed(&mut self) {
        self.slash_selected = 0;
    }

    /// Clamp the cursor to a valid char boundary inside `composer`.
    fn clamp_cursor(&mut self) {
        if self.cursor > self.composer.len() || !self.composer.is_char_boundary(self.cursor) {
            self.cursor = self.composer.len();
        }
    }

    /// Replace the composer text and park the cursor at its end.
    fn set_composer_text(&mut self, text: String) {
        self.cursor = text.len();
        self.composer = text;
    }

    pub fn clear_composer(&mut self) {
        self.composer.clear();
        self.cursor = 0;
        self.on_composer_changed();
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, ch: char) {
        self.clamp_cursor();
        self.composer.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// `shift+enter` / `alt+enter`: newline at the cursor position.
    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Backspace: delete the character before the cursor.
    pub fn backspace(&mut self) {
        self.clamp_cursor();
        if self.cursor == 0 {
            return;
        }
        let prev = self.composer[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.composer.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    /// Move the cursor one letter to the left.
    pub fn move_cursor_left(&mut self) {
        self.clamp_cursor();
        if self.cursor == 0 {
            return;
        }
        self.cursor = self.composer[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
    }

    /// Move the cursor one letter to the right.
    pub fn move_cursor_right(&mut self) {
        self.clamp_cursor();
        if let Some(ch) = self.composer[self.cursor..].chars().next() {
            self.cursor += ch.len_utf8();
        }
    }

    /// Word navigation: jump to the start of the previous word.
    pub fn move_word_left(&mut self) {
        self.clamp_cursor();
        let mut i = self.cursor;
        // Skip whitespace, then the word itself.
        while i > 0 {
            let Some((index, ch)) = self.composer[..i].char_indices().next_back() else {
                break;
            };
            if !ch.is_whitespace() {
                break;
            }
            i = index;
        }
        while i > 0 {
            let Some((index, ch)) = self.composer[..i].char_indices().next_back() else {
                break;
            };
            if ch.is_whitespace() {
                break;
            }
            i = index;
        }
        self.cursor = i;
    }

    /// Word navigation: jump to the start of the next word.
    pub fn move_word_right(&mut self) {
        self.clamp_cursor();
        let len = self.composer.len();
        let mut i = self.cursor;
        // Skip the current word, then any whitespace after it.
        while i < len {
            let Some(ch) = self.composer[i..].chars().next() else {
                break;
            };
            if ch.is_whitespace() {
                break;
            }
            i += ch.len_utf8();
        }
        while i < len {
            let Some(ch) = self.composer[i..].chars().next() else {
                break;
            };
            if !ch.is_whitespace() {
                break;
            }
            i += ch.len_utf8();
        }
        self.cursor = i;
    }

    /// Home: jump to the start of the composer.
    pub fn move_cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// End: jump to the end of the composer.
    pub fn move_cursor_end(&mut self) {
        self.cursor = self.composer.len();
    }

    pub fn paste(&mut self, text: &str) {
        self.clamp_cursor();
        let cleaned = text.replace("\r\n", "\n");
        self.composer.insert_str(self.cursor, &cleaned);
        self.cursor += cleaned.len();
        self.on_composer_changed();
    }

    pub fn recall_prev(&mut self) {
        if self.prompt_history.is_empty() {
            return;
        }
        match self.history_nav {
            None => {
                let index = self.prompt_history.len() - 1;
                self.history_nav = Some((index, self.composer.clone()));
                let text = self.prompt_history[index].clone();
                self.set_composer_text(text);
            }
            Some((index, ref draft)) => {
                if index > 0 {
                    self.history_nav = Some((index - 1, draft.clone()));
                    let text = self.prompt_history[index - 1].clone();
                    self.set_composer_text(text);
                }
            }
        }
    }

    pub fn recall_next(&mut self) {
        let Some((index, draft)) = self.history_nav.take() else {
            return;
        };
        if index + 1 < self.prompt_history.len() {
            self.history_nav = Some((index + 1, draft));
            let text = self.prompt_history[index + 1].clone();
            self.set_composer_text(text);
        } else {
            self.set_composer_text(draft);
        }
    }

    /// Tab: complete the highlighted slash command into the composer.
    pub fn accept_slash(&mut self) {
        let Some(name) = self
            .slash_filtered()
            .get(self.slash_selected)
            .map(|command| command.name)
        else {
            return;
        };
        self.set_composer_text(format!("{} ", name));
        self.on_composer_changed();
    }

    pub fn slash_up(&mut self) {
        let len = self.slash_filtered().len();
        if len > 0 {
            self.slash_selected = (self.slash_selected + len - 1) % len;
        }
    }

    pub fn slash_down(&mut self) {
        let len = self.slash_filtered().len();
        if len > 0 {
            self.slash_selected = (self.slash_selected + 1) % len;
        }
    }

    /// The slash popup is open while the composer starts with `/` and an
    /// unambiguous command prefix is being typed.
    pub fn slash_filtered(&self) -> Vec<&'static SlashCmd> {
        if self.overlay.is_some() {
            return Vec::new();
        }
        let text = self.composer.trim_start();
        if !text.starts_with('/') {
            return Vec::new();
        }
        let rest = &text[1..];
        if rest.contains(char::is_whitespace) {
            return Vec::new();
        }
        SLASH_COMMANDS
            .iter()
            .filter(|command| command.name[1..].starts_with(rest))
            .collect()
    }

    pub fn slash_open(&self) -> bool {
        !self.slash_filtered().is_empty()
    }

    /// `Enter`: dispatch the highlighted slash command, run a typed command,
    /// or send the message to the model.
    pub fn submit(&mut self) {
        if self.overlay.is_some() || self.streaming {
            return;
        }
        if self.slash_open() {
            let name = match self.slash_filtered().get(self.slash_selected) {
                Some(command) => command.name.to_string(),
                None => return,
            };
            self.composer.clear();
            self.cursor = 0;
            self.history_nav = None;
            self.run_command(&name);
            return;
        }
        let text = self.composer.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.composer.clear();
        self.cursor = 0;
        self.history_nav = None;
        if text.starts_with('/') {
            self.run_command(&text);
            return;
        }
        self.prompt_history.push(text.clone());
        self.cells.push(Cell::User(text.clone()));
        self.sessions.add_message("user", text);
        if let Err(error) = self.start_stream() {
            self.push_error(error.to_string());
        }
    }

    fn run_command(&mut self, text: &str) {
        let mut parts = text.splitn(2, char::is_whitespace);
        let command = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        let argument = parts.next().unwrap_or("").trim().to_string();
        match command.as_str() {
            "/help" => self.overlay = Some(Overlay::Shortcuts),
            "/new" => self.new_chat(),
            "/history" => self.open_history(),
            "/code" => self.open_code(),
            "/model" => {
                if argument.is_empty() {
                    self.push_error("usage: /model <model-id>".into());
                } else {
                    self.config.model = argument.clone();
                    self.push_notice(format!("model set to {argument}"));
                }
            }
            "/quit" => self.should_quit = true,
            other => self.push_error(format!("unknown command: {other} — try /help")),
        }
    }

    pub fn new_chat(&mut self) {
        if self.sessions.current().messages.is_empty() {
            self.push_notice("already in a new conversation".into());
            return;
        }
        self.sessions.new_session();
        self.rebuild_cells();
        self.scroll_from_bottom = 0;
    }

    // -- quit flow --------------------------------------------------------------

    pub fn prime_quit(&mut self) {
        if self.quit_primed() {
            self.should_quit = true;
        } else {
            self.quit_primed_at = Some(Instant::now());
        }
    }

    pub fn quit_primed(&self) -> bool {
        self.quit_primed_at
            .is_some_and(|at| at.elapsed() < QUIT_PRIME_WINDOW)
    }

    // -- streaming ----------------------------------------------------------------

    pub fn elapsed_secs(&self) -> u64 {
        self.stream_started
            .map(|started| started.elapsed().as_secs())
            .unwrap_or(0)
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.stream_started
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }

    fn start_stream(&mut self) -> Result<()> {
        let Some(api_key) = self.config.api_key.clone() else {
            return Err(anyhow::anyhow!(
                "DAHL_API_KEY is not configured — set it in dahl.json or the environment"
            ));
        };
        let (tx, rx) = mpsc::channel(64);
        let messages: Vec<(String, String)> = self
            .sessions
            .current()
            .messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();
        let model = self.config.model.clone();
        let temperature = self.config.temperature;
        let client = ApiClient::new(api_key, self.config.base_url.clone());
        tokio::spawn(async move {
            if let Err(error) = client
                .stream_chat(&messages, &model, temperature, tx.clone())
                .await
            {
                let _ = tx.send(Err(error)).await;
            }
        });
        self.tokens = Some(rx);
        self.streaming = true;
        self.stream_started = Some(Instant::now());
        Ok(())
    }

    pub async fn receive_token(&mut self) {
        let Some(mut rx) = self.tokens.take() else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(Ok(token)) => self.response.push_str(&token),
                Ok(Err(error)) => {
                    self.finish_partial();
                    self.streaming = false;
                    self.stream_started = None;
                    self.push_error(error.to_string());
                    return;
                }
                Err(TryRecvError::Empty) => {
                    self.tokens = Some(rx);
                    return;
                }
                Err(TryRecvError::Disconnected) => break,
            }
        }
        self.streaming = false;
        self.stream_started = None;
        self.finish_partial();
    }

    /// Commit an in-flight assistant response (used on completion and interrupt).
    fn finish_partial(&mut self) {
        if !self.response.is_empty() {
            let text = std::mem::take(&mut self.response);
            self.cells.push(Cell::Assistant(text.clone()));
            self.sessions.add_message("assistant", text);
        }
    }

    fn push_error(&mut self, message: String) {
        self.cells.push(Cell::Error(message));
    }

    fn push_notice(&mut self, message: String) {
        self.cells.push(Cell::Notice(message));
    }

    /// Right-hand footer summary, codex-style context indicator.
    pub fn context_summary(&self) -> String {
        let session = self.sessions.current();
        let messages = session.messages.len();
        let chars: usize = session.messages.iter().map(|m| m.content.len()).sum();
        format!(
            "{messages} msgs · ~{} tok",
            crate::ui::theme::human_tokens(chars / 4)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        App::new(Config::default(), SessionManager::for_tests())
    }

    #[test]
    fn submit_routes_slash_commands() {
        let mut app = test_app();
        app.composer = "/model test-model".into();
        app.submit();
        assert_eq!(app.config.model, "test-model");
        assert!(matches!(app.cells.last(), Some(Cell::Notice(_))));
        assert!(app.composer.is_empty());
    }

    #[test]
    fn slash_popup_filters_by_prefix() {
        let mut app = test_app();
        app.composer = "/m".into();
        let names: Vec<&str> = app.slash_filtered().iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["/model"]);
        app.composer = "/model something".into();
        assert!(!app.slash_open());
    }

    #[test]
    fn unknown_command_is_reported_not_sent() {
        let mut app = test_app();
        app.composer = "/nope".into();
        app.submit();
        assert!(matches!(app.cells.last(), Some(Cell::Error(_))));
        assert!(app.sessions.current().messages.is_empty());
    }

    #[test]
    fn prompt_history_recalls_and_restores_draft() {
        let mut app = test_app();
        app.composer = "first".into();
        app.prompt_history.push("first".into());
        app.composer = "draft".into();
        app.recall_prev();
        assert_eq!(app.composer, "first");
        app.recall_next();
        assert_eq!(app.composer, "draft");
    }

    #[test]
    fn escape_closes_overlay_before_stream() {
        let mut app = test_app();
        app.overlay = Some(Overlay::Shortcuts);
        app.escape();
        assert!(app.overlay.is_none());
        app.streaming = true;
        app.response = "partial".into();
        app.escape();
        assert!(!app.streaming);
        assert!(matches!(app.cells.last(), Some(Cell::Assistant(_))));
    }

    #[test]
    fn quit_needs_double_ctrl_c() {
        let mut app = test_app();
        app.prime_quit();
        assert!(!app.should_quit);
        app.prime_quit();
        assert!(app.should_quit);
    }

    #[test]
    fn letter_navigation_moves_one_char_at_a_time() {
        let mut app = test_app();
        app.set_composer_text("aé b".into()); // 'é' is 2 bytes, len = 5
        assert_eq!(app.cursor, 5);
        app.move_cursor_left();
        assert_eq!(app.cursor, 4); // before 'b'
        app.move_cursor_left();
        assert_eq!(app.cursor, 3); // before ' '
        app.move_cursor_left();
        assert_eq!(app.cursor, 1); // before 'é', on its byte boundary
        app.move_cursor_left();
        assert_eq!(app.cursor, 0);
        app.move_cursor_left();
        assert_eq!(app.cursor, 0); // stays at the start
        app.move_cursor_right();
        assert_eq!(app.cursor, 1);
        app.move_cursor_right();
        assert_eq!(app.cursor, 3); // 'é' is skipped as one letter
        app.move_cursor_right();
        app.move_cursor_right();
        app.move_cursor_right();
        assert_eq!(app.cursor, 5); // stays at the end
    }

    #[test]
    fn word_navigation_jumps_between_words() {
        let mut app = test_app();
        app.set_composer_text("hello brave new world".into());
        app.move_word_left();
        assert_eq!(app.cursor, 16); // start of "world"
        app.move_word_left();
        assert_eq!(app.cursor, 12); // start of "new"
        app.move_word_right();
        assert_eq!(app.cursor, 16); // end of "new"
        app.move_word_right();
        assert_eq!(app.cursor, 21); // end of "world"
        app.move_word_right();
        assert_eq!(app.cursor, 21); // stays at the end
        app.move_cursor_home();
        assert_eq!(app.cursor, 0);
        app.move_cursor_end();
        assert_eq!(app.cursor, 21);
    }

    #[test]
    fn editing_happens_at_cursor() {
        let mut app = test_app();
        app.set_composer_text("hello world".into());
        app.move_word_left(); // cursor at 6, before "world"
        app.insert_char('X');
        assert_eq!(app.composer, "hello Xworld");
        assert_eq!(app.cursor, 7);
        app.backspace();
        assert_eq!(app.composer, "hello world");
        assert_eq!(app.cursor, 6);
        app.paste("pasted\n");
        assert_eq!(app.composer, "hello pasted\nworld");
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
