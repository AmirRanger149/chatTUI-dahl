//! Clipboard support for copying code blocks. Prefers a platform clipboard
//! utility (so success can actually be verified), and falls back to the
//! terminal's OSC 52 escape sequence, which most modern terminals honor.

use anyhow::Result;
use std::io::Write;
use std::process::{Command, Stdio};

/// Copy `text` to the clipboard. Returns a short description of the method
/// that was used, for the confirmation notice.
pub fn copy(text: &str) -> Result<String> {
    for argv in platform_commands() {
        if let Some(method) = try_command(&argv, text) {
            return Ok(method);
        }
    }
    write_osc52(text)?;
    Ok("terminal clipboard (OSC 52)".to_string())
}

/// Candidate clipboard utilities per platform, in preference order.
fn platform_commands() -> Vec<Vec<String>> {
    if cfg!(target_os = "macos") {
        vec![vec!["pbcopy".to_string()]]
    } else if cfg!(target_os = "windows") {
        vec![vec!["clip".to_string()]]
    } else {
        let mut candidates = Vec::new();
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            candidates.push(vec!["wl-copy".to_string()]);
        }
        candidates.push(vec![
            "xclip".to_string(),
            "-selection".to_string(),
            "clipboard".to_string(),
        ]);
        candidates.push(vec![
            "xsel".to_string(),
            "--clipboard".to_string(),
            "--input".to_string(),
        ]);
        candidates.push(vec!["wl-copy".to_string()]);
        candidates
    }
}

/// Pipe `text` into a clipboard utility; report the method on success.
fn try_command(argv: &[String], text: &str) -> Option<String> {
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            let _ = child.kill();
            return None;
        }
    }
    match child.wait() {
        Ok(status) if status.success() => Some(format!("system clipboard ({})", argv[0])),
        _ => None,
    }
}

/// OSC 52: ask the terminal itself to store the text in the system clipboard.
fn write_osc52(text: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    write!(stdout, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
    stdout.flush()?;
    Ok(())
}

/// Minimal standard base64 encoder — avoids pulling in an extra dependency.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let n = ((chunk[0] as u32) << 16)
            | ((*chunk.get(1).unwrap_or(&0) as u32) << 8)
            | (*chunk.get(2).unwrap_or(&0) as u32);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
