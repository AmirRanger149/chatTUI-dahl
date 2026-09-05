use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc::Sender;

pub struct ApiClient {
    http: Client,
    api_key: String,
    base_url: String,
}

impl ApiClient {
    pub fn new(api_key: String, base_url: String) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                // Fail hung connections quickly, but do not cap the total
                // duration of an active generation.
                .connect_timeout(Duration::from_secs(20))
                .read_timeout(Duration::from_secs(180))
                .build()
                .context("building HTTP client")?,
            api_key,
            base_url: base_url.trim_end_matches('/').into(),
        })
    }

    pub async fn stream_chat(
        &self,
        messages: &[(String, String)],
        model: &str,
        temperature: f32,
        tx: Sender<Result<String>>,
    ) -> Result<()> {
        let mut openai_messages = Vec::new();
        for (role, content) in messages {
            let role = match role.as_str() {
                "system" | "assistant" | "user" => role.as_str(),
                _ => "user",
            };
            openai_messages.push(json!({ "role": role, "content": content }));
        }
        let body = json!({
            "model": model,
            "messages": openai_messages,
            "temperature": temperature,
            "stream": true,
        });
        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|err| map_transport_error(err, &self.base_url, &self.api_key))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(http_error(status, &body, &self.api_key));
        }
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        loop {
            match stream.next().await {
                Some(Ok(chunk)) => {
                    buffer.extend_from_slice(&chunk);
                    if drain_sse_lines(&mut buffer, &tx).await? {
                        return Ok(());
                    }
                }
                Some(Err(err)) => {
                    return Err(map_transport_error(err, &self.base_url, &self.api_key));
                }
                None => {
                    if !buffer.iter().all(u8::is_ascii_whitespace) {
                        buffer.push(b'\n');
                        drain_sse_lines(&mut buffer, &tx).await?;
                    }
                    return Ok(());
                }
            }
        }
    }
}

/// Process complete SSE lines in `buffer`. Returns true when the stream is
/// finished (`[DONE]`) or the UI dropped the receiver.
async fn drain_sse_lines(buffer: &mut Vec<u8>, tx: &Sender<Result<String>>) -> Result<bool> {
    while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
        let line: Vec<u8> = buffer.drain(..=end).collect();
        if handle_sse_line(&line, tx).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn handle_sse_line(line: &[u8], tx: &Sender<Result<String>>) -> Result<bool> {
    let Some(data) = sse_data(line)? else {
        return Ok(false);
    };
    if data.is_empty() {
        return Ok(false);
    }
    if data == "[DONE]" {
        return Ok(true);
    }
    match parse_stream_payload(data)? {
        Some(text) => {
            if tx.send(Ok(text)).await.is_err() {
                // The user interrupted the stream; stop quietly.
                return Ok(true);
            }
        }
        None => {}
    }
    Ok(false)
}

/// Parse one SSE `data:` payload.
///
/// * `Ok(Some(text))` — content to append
/// * `Ok(None)` — expected non-content event (role, finish_reason, empty choices)
/// * `Err` — malformed JSON or an explicit API error object
fn parse_stream_payload(data: &str) -> Result<Option<String>> {
    let value: Value = serde_json::from_str(data)
        .map_err(|err| anyhow!("malformed streaming JSON: {err}"))?;
    if let Some(message) = error_message_from_json(&value) {
        return Err(anyhow!("API error: {message}"));
    }
    Ok(content_from_chunk(&value))
}

fn content_from_chunk(value: &Value) -> Option<String> {
    let choices = value.get("choices")?.as_array()?;
    let mut out = String::new();
    for choice in choices {
        if let Some(text) = choice
            .get("delta")
            .and_then(|delta| delta.get("content"))
            .and_then(Value::as_str)
        {
            out.push_str(text);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn sse_data(line: &[u8]) -> Result<Option<&str>> {
    let text = match std::str::from_utf8(line) {
        Ok(text) => text,
        Err(_) => {
            if line.iter().all(|byte| byte.is_ascii_whitespace()) {
                return Ok(None);
            }
            return Err(anyhow!("malformed streaming event: invalid UTF-8"));
        }
    };
    let text = text.trim_end_matches(['\r', '\n']);
    if text.is_empty() || text.starts_with(':') {
        return Ok(None);
    }
    Ok(text.strip_prefix("data:").map(str::trim))
}

fn error_message_from_json(value: &Value) -> Option<String> {
    if let Some(error) = value.get("error") {
        if let Some(message) = error.as_str().and_then(nonempty_text) {
            return Some(message);
        }
        if let Some(message) = error
            .get("message")
            .and_then(Value::as_str)
            .and_then(nonempty_text)
        {
            return Some(message);
        }
    }
    value
        .get("message")
        .and_then(Value::as_str)
        .and_then(nonempty_text)
}

fn nonempty_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn http_error(status: StatusCode, body: &str, api_key: &str) -> anyhow::Error {
    let body = redact(body, api_key);
    let detail = error_message_from_json(&serde_json::from_str(&body).unwrap_or(Value::Null))
        .or_else(|| fallback_body_detail(&body));
    let hint = match status.as_u16() {
        400 => "bad request",
        401 => "unauthorized — check DAHL_API_KEY",
        403 => "forbidden — this API key may not have access",
        404 => "not found — check the base URL and model name",
        429 => "rate limited — wait and retry",
        500..=599 => "server error — try again later",
        _ => "request failed",
    };
    match detail {
        Some(detail) => anyhow!("HTTP {status} {hint}: {detail}"),
        None => anyhow!("HTTP {status} {hint}"),
    }
}

fn fallback_body_detail(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() || trimmed.starts_with('<') {
        return None;
    }
    Some(trimmed.chars().take(240).collect())
}

fn map_transport_error(err: reqwest::Error, base_url: &str, api_key: &str) -> anyhow::Error {
    let message = redact(&err.to_string(), api_key);
    if err.is_timeout() {
        anyhow!("timed out connecting to the API or waiting for the next token")
    } else if err.is_connect() {
        anyhow!("could not connect to {base_url}: {message}")
    } else {
        anyhow!("chat request failed: {message}")
    }
}

fn redact(text: &str, api_key: &str) -> String {
    if api_key.is_empty() {
        text.to_string()
    } else {
        text.replace(api_key, "***")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn extracts_openai_sse_data() {
        assert_eq!(
            sse_data(b"data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\r\n").unwrap(),
            Some("{\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}")
        );
    }

    #[test]
    fn ignores_non_data_sse_lines() {
        assert_eq!(sse_data(b"event: message\n").unwrap(), None);
        assert_eq!(sse_data(b"\n").unwrap(), None);
        assert_eq!(sse_data(b": keepalive\n").unwrap(), None);
        assert_eq!(sse_data(b"data:   \n").unwrap(), Some(""));
    }

    #[test]
    fn content_delta_is_emitted() {
        let payload = r#"{"choices":[{"delta":{"content":"Hello"}}]}"#;
        assert_eq!(parse_stream_payload(payload).unwrap().as_deref(), Some("Hello"));
    }

    #[test]
    fn empty_choices_and_missing_delta_are_skipped() {
        assert_eq!(parse_stream_payload(r#"{"choices":[]}"#).unwrap(), None);
        assert_eq!(
            parse_stream_payload(r#"{"choices":[{"finish_reason":"stop"}]}"#).unwrap(),
            None
        );
        assert_eq!(
            parse_stream_payload(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#).unwrap(),
            None
        );
        assert_eq!(
            parse_stream_payload(r#"{"choices":[{"delta":{"content":null},"finish_reason":"stop"}]}"#)
                .unwrap(),
            None
        );
        assert_eq!(parse_stream_payload(r#"{"id":"cmpl-1"}"#).unwrap(), None);
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_stream_payload("{not json").is_err());
    }

    #[test]
    fn api_error_object_is_an_error() {
        let err = parse_stream_payload(r#"{"error":{"message":"model not found"}}"#).unwrap_err();
        assert!(err.to_string().contains("model not found"));
    }

    #[test]
    fn http_errors_include_status_and_body_message() {
        let err = http_error(
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"invalid api key sk-secret"}}"#,
            "sk-secret",
        );
        let text = err.to_string();
        assert!(text.contains("401"));
        assert!(text.contains("unauthorized"));
        assert!(text.contains("invalid api key"));
        assert!(!text.contains("sk-secret"));
    }

    #[test]
    fn http_429_and_500_have_distinct_hints() {
        assert!(http_error(StatusCode::TOO_MANY_REQUESTS, "", "")
            .to_string()
            .contains("rate limited"));
        assert!(http_error(StatusCode::INTERNAL_SERVER_ERROR, "", "")
            .to_string()
            .contains("server error"));
        assert!(http_error(StatusCode::NOT_FOUND, "", "")
            .to_string()
            .contains("not found"));
    }
}
