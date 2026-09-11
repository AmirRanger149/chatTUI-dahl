//! An OpenAI-compatible chat backend. Covers OpenAI itself and every gateway
//! that speaks the `POST /chat/completions` dialect (custom providers such
//! as Dahl, APInex, Ollama, Groq, Mistral, Together, Azure OpenAI, …). This
//! is the workhorse backend: every `custom_providers` entry in `config.json`
//! uses it.

use super::ChatBackend;
use crate::api::error::{classify_failure, error_detail};
use crate::api::sse::SseReader;
use crate::api::types::{CompletionRequest, Failure, Role, StreamEvent};
use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

pub struct OpenAICompatibleBackend {
    http: Client,
    api_key: String,
    base_url: String,
}

impl OpenAICompatibleBackend {
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

    async fn list_models_inner(&self) -> Result<Vec<String>> {
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

    async fn stream_inner(
        &self,
        request: CompletionRequest,
        tx: Sender<StreamEvent>,
    ) -> Result<(), Failure> {
        let mut openai_messages = Vec::with_capacity(request.messages.len());
        for message in &request.messages {
            let role = match message.role {
                Role::System => "system",
                Role::Assistant => "assistant",
                Role::User => "user",
            };
            openai_messages.push(json!({ "role": role, "content": message.content }));
        }
        let body = json!({
            "model": request.model,
            "messages": openai_messages,
            "temperature": request.temperature,
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
        let mut reader = SseReader::new();
        let mut emitted = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                if emitted {
                    Failure::Fatal(format!("stream interrupted after output started: {error}"))
                } else {
                    Failure::Retryable(format!("stream interrupted before any output: {error}"))
                }
            })?;
            for data in reader.feed(&chunk) {
                if data == "[DONE]" {
                    return Ok(());
                }
                let value: Value = serde_json::from_str(&data)
                    .map_err(|error| Failure::Fatal(format!("parsing streaming response failed: {error}")))?;
                if let Some(text) = value["choices"][0]["delta"]["content"].as_str() {
                    tx.send(StreamEvent::Delta(text.into()))
                        .await
                        .map_err(|_| Failure::Fatal("stream receiver closed".into()))?;
                    emitted = true;
                } else if let Some(message) = value["error"]["message"].as_str() {
                    // Some gateways report failures inside a 200-OK stream.
                    if emitted {
                        return Err(Failure::Fatal(format!("API error mid-stream: {message}")));
                    }
                    return Err(classify_failure(0, message));
                }
            }
        }
        Ok(())
    }
}

impl ChatBackend for OpenAICompatibleBackend {
    fn list_models(&self) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + '_>> {
        Box::pin(self.list_models_inner())
    }

    fn stream_completion(
        &self,
        request: &CompletionRequest,
        tx: Sender<StreamEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Failure>> + Send + '_>> {
        Box::pin(self.stream_inner(request.clone(), tx))
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

#[cfg(test)]
mod tests {
    use super::*;

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
