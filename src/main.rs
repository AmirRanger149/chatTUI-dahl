mod api;
mod app;
mod config;
mod session;
mod ui;

use anyhow::Result;
use app::App;
use config::Config;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use session::manager::SessionManager;
use std::io::{self, stdout};
use tokio::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let sessions = SessionManager::load()?;
    let mut app = App::new(config, sessions);
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, &mut app).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        app.receive_token().await;
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(50))? {
            loop {
                match event::read()? {
                    Event::Key(key) => {
                        if !handle_key(app, key) {
                            return Ok(());
                        }
                    }
                    Event::Paste(text) => app.paste(&text),
                    _ => {}
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if app.should_quit {
            return Ok(());
        }
    }
}

/// Returns `false` when the app should exit.
fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    // ctrl+c: press twice within 2s to quit, from any state.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.prime_quit();
        return !app.should_quit;
    }

    // Global keys.
    match key.code {
        KeyCode::Esc => {
            app.escape();
            return true;
        }
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.toggle_history();
            return true;
        }
        KeyCode::PageUp => {
            app.scroll(-10);
            return true;
        }
        KeyCode::PageDown => {
            app.scroll(10);
            return true;
        }
        _ => {}
    }

    // Overlays consume remaining keys.
    match app.overlay {
        Some(app::Overlay::Shortcuts) => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?')) {
                app.overlay = None;
            }
            return true;
        }
        Some(app::Overlay::History { .. }) => {
            match key.code {
                KeyCode::Up => app.move_history_selection(-1),
                KeyCode::Down => app.move_history_selection(1),
                KeyCode::Enter => app.load_selected_session(),
                KeyCode::Char('d') => app.delete_selected_session(),
                _ => {}
            }
            return true;
        }
        None => {}
    }

    // The composer is always active, like codex.
    match key.code {
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.composer.push('\n');
            app.on_composer_changed();
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => {
            app.composer.push('\n');
            app.on_composer_changed();
        }
        KeyCode::Enter => app.submit(),
        KeyCode::Tab => app.accept_slash(),
        KeyCode::Backspace => {
            app.composer.pop();
            app.on_composer_changed();
        }
        KeyCode::Up if app.slash_open() => app.slash_up(),
        KeyCode::Down if app.slash_open() => app.slash_down(),
        KeyCode::Up => app.recall_prev(),
        KeyCode::Down => app.recall_next(),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.composer.clear();
            app.on_composer_changed();
        }
        KeyCode::Char('?') if app.composer.is_empty() => app.toggle_shortcuts(),
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            app.composer.push(ch);
            app.on_composer_changed();
        }
        _ => {}
    }
    true
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, event::EnableBracketedPaste)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout()))?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, event::DisableBracketedPaste)?;
    terminal.show_cursor()?;
    Ok(())
}
