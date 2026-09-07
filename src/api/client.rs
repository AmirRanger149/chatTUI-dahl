use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

/// Events streamed back from a chat request. Errors end the stream; notices
/// are informational rows (e.g. a model-fallback announcement) that appear in
/// the transcript while the stream keeps going.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// A piece of assistant text.
    Delta(String),
    /// Progress information, shown as a quiet transcript row.
    Notice(String),
    /// The request failed for good.
    Error(String),
}

/// Why a chat attempt failed, and whether a different model could fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Another model may succeed: bad requests for this model, missing or
    /// decommissioned models, "high demand"/overload, throttling, upstream
    /// outages.
    Retryable(String),
    /// Switching models will not help: rejected credentials, protocol
    /// errors, a stream that broke after output started.
    Fatal(String),
}

/// How many models a single message may try before giving up: the requested
/// model plus up to three fallbacks.
pub const MAX_MODELS_PER_SEND: usize = 4;

pub struct ApiClient {
    http: Client,
    api_key: String,
    base_url: String,
}

impl ApiClient {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(90))
                .build()
                .expect("HTTP client"),
            api_key,
            base_url: base_url.trim_end_matches('/').into(),
        }
    }

    /// The model ids the endpoint currently offers (`GET /models`),
    /// sorted case-insensitively.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/models", self.base_url);
        let response = self
            .http
            .get(url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .context("sending GET /models request")?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!(
                "model list request failed (HTTP {}): {}",
                status,
                error_detail(&response.text().await.unwrap_or_default())
            ));
        }
        let value: Value = response
            .json()
            .await
            .context("parsing the model list response")?;
        let mut ids = parse_models(&value);
        ids.sort_by_key(|id| id.to_ascii_lowercase());
        if ids.is_empty() {
            return Err(anyhow!("the endpoint returned no models"));
        }
        Ok(ids)
    }

    /// Stream a chat completion, falling back to other models the endpoint
    /// offers when the requested one rejects the request or is at high
    /// demand. Every switch is announced as a [`StreamEvent::Notice`] so the
    /// transcript can tell the user which model actually answered.
    pub async fn stream_chat(
        &self,
        messages: &[(String, String)],
        model: &str,
        temperature: f32,
        tx: Sender<StreamEvent>,
    ) -> Result<()> {
        let mut tried: Vec<String> = vec![model.to_string()];
        let mut fallbacks: Option<Vec<String>> = None;
        // Whether any of the answer reached the transcript: once it has,
        // splicing in a second model would produce one Frankenstein reply.
        let mut emitted = false;

        loop {
            let current = tried.last().cloned().unwrap_or_default();
            match self
                .attempt(messages, &current, temperature, &tx, &mut emitted)
                .await
            {
                Ok(()) => return Ok(()),
                Err(Failure::Fatal(reason)) => return Err(anyhow!(reason)),
                Err(Failure::Retryable(reason)) => {
                    if emitted {
                        return Err(anyhow!(
                            "{reason} — a partial answer was already received, so no fallback was attempted"
                        ));
                    }
                    if fallbacks.is_none() {
                        let _ = tx
                            .send(StreamEvent::Notice(format!(
                                "{current} is unavailable — {reason}"
                            )))
                            .await;
                        fallbacks = Some(match self.list_models().await {
                            Ok(models) => order_fallbacks(models, &current),
                            Err(list_error) => {
                                return Err(anyhow!(
                                    "{current} is unavailable — {reason}; fetching the fallback model list failed too ({list_error})"
                                ));
                            }
                        });
                    }
                    let list = fallbacks.as_ref().expect("fallback list loaded above");
                    let Some(next) = list.iter().find(|id| !tried.contains(*id)) else {
                        return Err(anyhow!(
                            "all {} available model(s) failed — last error: {reason}",
                            tried.len()
                        ));
                    };
                    if tried.len() >= MAX_MODELS_PER_SEND {
                        return Err(anyhow!(
                            "tried {} models without success — last error: {reason}",
                            tried.len()
                        ));
                    }
                    let next = next.clone();
                    let _ = tx
                        .send(StreamEvent::Notice(format!("switching to {next}")))
                        .await;
                    tried.push(next);
                }
            }
        }
    }

    /// One streaming attempt against one model.
    async fn attempt(
        &self,
        messages: &[(String, String)],
        model: &str,
        temperature: f32,
        tx: &Sender<StreamEvent>,
        emitted: &mut bool,
    ) -> Result<(), Failure> {
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
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| Failure::Retryable(format!("request failed: {error}")))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(classify_failure(status.as_u16(), &body));
        }
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                if *emitted {
                    Failure::Fatal(format!("stream interrupted after output started: {error}"))
                } else {
                    Failure::Retryable(format!("stream interrupted before any output: {error}"))
                }
            })?;
            buffer.extend_from_slice(&chunk);
            while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
                let line = buffer.drain(..=end).collect::<Vec<_>>();
                let Some(data) = sse_data(&line) else {
                    continue;
                };
                if data == "[DONE]" {
                    return Ok(());
                }
                let value: Value = serde_json::from_str(data)
                    .map_err(|error| Failure::Fatal(format!("parsing streaming response failed: {error}")))?;
                if let Some(text) = value["choices"][0]["delta"]["content"].as_str() {
                    tx.send(StreamEvent::Delta(text.into()))
                        .await
                        .map_err(|_| Failure::Fatal("stream receiver closed".into()))?;
                    *emitted = true;
                } else if let Some(message) = value["error"]["message"].as_str() {
                    // Some gateways report failures inside a 200-OK stream.
                    if *emitted {
                        return Err(Failure::Fatal(format!("API error mid-stream: {message}")));
                    }
                    return Err(classify_failure(0, message));
                }
            }
        }
        Ok(())
    }
}

/// Pull model ids out of an OpenAI-compatible `GET /models` response: either
/// `{"data": [{"id": …}, …]}` or a bare `[{"id": …}, …]` array.
fn parse_models(value: &Value) -> Vec<String> {
    let entries: &[Value] = match value {
        Value::Array(items) => items,
        value => value["data"].as_array().map(Vec::as_slice).unwrap_or(&[]),
    };
    entries
        .iter()
        .filter_map(|item| item["id"].as_str().map(str::to_string))
        .collect()
}

/// Decide whether a failed chat response could succeed with a different
/// model. `status` is the HTTP status, or `0` when the API failed inside a
/// 200-OK stream and only the error text is known.
fn classify_failure(status: u16, body: &str) -> Failure {
    let lower = body.to_ascii_lowercase();

    // Wrong credentials are fatal no matter which model is asked.
    const AUTH: &[&str] = &[
        "invalid api key",
        "invalid_api_key",
        "incorrect api key",
        "missing api key",
        "unauthorized",
        "authentication",
        "not authenticated",
        "forbidden",
    ];
    if AUTH.iter().any(|phrase| lower.contains(phrase)) {
        return Failure::Fatal(compact_failure("the API rejected the credentials", status, body));
    }

    // These statuses are the API saying "not this model / not right now":
    // bad requests against the model, missing models, throttling and
    // upstream outages. A different model may well work.
    if matches!(status, 400 | 404 | 408 | 409 | 413 | 422 | 425 | 429)
        || (500..=599).contains(&status)
    {
        return Failure::Retryable(compact_failure("the model rejected the request", status, body));
    }

    // Other statuses can still mean "model busy / gone" by their text —
    // the classic "currently experiencing high demand" line.
    const MODEL_TROUBLE: &[&str] = &[
        "high demand",
        "overload",
        "at capacity",
        "capacity",
        "busy",
        "rate limit",
        "too many requests",
        "quota",
        "temporarily unavailable",
        "unavailable",
        "try again",
        "model not found",
        "does not exist",
        "invalid model",
        "not a valid model",
        "no longer available",
        "not available",
        "decommissioned",
        "deprecated",
    ];
    if MODEL_TROUBLE.iter().any(|phrase| lower.contains(phrase)) {
        return Failure::Retryable(compact_failure("the model is not available", status, body));
    }

    Failure::Fatal(compact_failure("the request failed", status, body))
}

/// One-line reason for a failed attempt: prefer the API's own error message,
/// fall back to the raw body, and keep it short.
fn compact_failure(label: &str, status: u16, body: &str) -> String {
    let detail = error_detail(body);
    if status == 0 {
        format!("{label}: {detail}")
    } else {
        format!("{label} (HTTP {status}: {detail})")
    }
}

/// Extract a short error message from a failed response body: use the
/// OpenAI-style `error.message` when present, otherwise the trimmed body.
fn error_detail(body: &str) -> String {
    let trimmed = body.trim();
    let detail = serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|value| {
            value["error"]["message"]
                .as_str()
                .or_else(|| value["message"].as_str())
                .or_else(|| value["detail"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| trimmed.to_string());
    truncate_chars(detail.trim(), 200)
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

/// Rank the models left after a failure: models from the same family (the
/// namespace before the `/`, e.g. `MiniMaxAI/…`) first, since a sibling
/// model is the likeliest drop-in replacement; everything else keeps the
/// API's own order. The failed model and duplicates are dropped.
fn order_fallbacks(models: Vec<String>, failed: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let family = failed.split('/').next().unwrap_or("").to_ascii_lowercase();
    let mut kin = Vec::new();
    let mut rest = Vec::new();
    for id in models {
        if id.eq_ignore_ascii_case(failed) || !seen.insert(id.clone()) {
            continue;
        }
        let same_family = !family.is_empty()
            && id.split('/').next().unwrap_or("").eq_ignore_ascii_case(&family);
        if same_family {
            kin.push(id);
        } else {
            rest.push(id);
        }
    }
    kin.extend(rest);
    kin
}

fn sse_data(line: &[u8]) -> Option<&str> {
    std::str::from_utf8(line)
        .ok()?
        .trim_end_matches(['\r', '\n'])
        .strip_prefix("data:")
        .map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_sse_data() {
        assert_eq!(
            sse_data(b"data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\r\n"),
            Some("{\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}")
        );
    }

    #[test]
    fn ignores_non_data_sse_lines() {
        assert_eq!(sse_data(b"event: message\n"), None);
        assert_eq!(sse_data(b"\n"), None);
    }

    #[test]
    fn classifies_retryable_and_fatal_failures() {
        // Bad request for the model → try another one.
        assert!(matches!(
            classify_failure(400, "{\"error\":{\"message\":\"Model not found\"}}"),
            Failure::Retryable(_)
        ));
        // The classic "high demand" overload line.
        assert!(matches!(
            classify_failure(503, "The model is currently experiencing high demand"),
            Failure::Retryable(_)
        ));
        assert!(matches!(classify_failure(429, "slow down"), Failure::Retryable(_)));
        assert!(matches!(classify_failure(500, "oops"), Failure::Retryable(_)));
        // Credentials never get better by switching models.
        assert!(matches!(
            classify_failure(401, "{\"error\":{\"message\":\"Invalid API key\"}}"),
            Failure::Fatal(_)
        ));
        assert!(matches!(classify_failure(405, "method not allowed"), Failure::Fatal(_)));
        // Mid-stream failure texts follow the same rules.
        assert!(matches!(
            classify_failure(0, "Model is currently getting high demand, try later"),
            Failure::Retryable(_)
        ));
        assert!(matches!(classify_failure(0, "unknown Explosion"), Failure::Fatal(_)));
    }

    #[test]
    fn fallback_order_prefers_the_same_family() {
        let ordered = order_fallbacks(
            vec![
                "Other/Model".into(),
                "Minimaxai/MiniMax-M1".into(),
                "MiniMaxAI/MiniMax-M2.7".into(),
                "Other/Model".into(),
                "MiniMaxAI/MiniMax-M2.7".into(),
            ],
            "MiniMaxAI/MiniMax-M3",
        );
        assert_eq!(
            ordered,
            vec![
                "Minimaxai/MiniMax-M1",
                "MiniMaxAI/MiniMax-M2.7",
                "Other/Model"
            ]
        );
    }

    #[test]
    fn error_detail_prefers_the_api_message() {
        assert_eq!(error_detail("{\"error\":{\"message\":\"boom\"}}"), "boom");
        assert_eq!(error_detail("{\"detail\":\"detailed\"}"), "detailed");
        assert_eq!(error_detail("  plain text  "), "plain text");
        assert_eq!(error_detail(""), "");
        assert!(error_detail(&"x".repeat(500)).chars().count() <= 201);
    }

    #[test]
    fn parses_openai_and_bare_model_lists() {
        let openai = serde_json::json!({
            "object": "list",
            "data": [{"id": "b"}, {"id": "a"}, {"object": "model"}]
        });
        assert_eq!(parse_models(&openai), vec!["b", "a"]);
        let bare = serde_json::json!([{"id": "y"}, {"id": "x"}]);
        assert_eq!(parse_models(&bare), vec!["y", "x"]);
        assert!(parse_models(&serde_json::json!({})).is_empty());
    }
}
