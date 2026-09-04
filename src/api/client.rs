use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc::Sender;

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
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("sending OpenAI-compatible chat request")?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!(
                "Chat request failed (HTTP {}): {}",
                status,
                response.text().await.unwrap_or_default()
            ));
        }
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        while let Some(chunk) = stream.next().await {
            buffer.extend_from_slice(&chunk?);
            while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
                let line = buffer.drain(..=end).collect::<Vec<_>>();
                let Some(data) = sse_data(&line) else {
                    continue;
                };
                if data == "[DONE]" {
                    return Ok(());
                }
                let value: Value = serde_json::from_str(data)
                    .context("parsing OpenAI-compatible streaming response")?;
                if let Some(text) = value["choices"][0]["delta"]["content"].as_str() {
                    tx.send(Ok(text.into()))
                        .await
                        .map_err(|_| anyhow!("stream receiver closed"))?;
                } else if let Some(message) = value["error"]["message"].as_str() {
                    return Err(anyhow!("API error: {message}"));
                }
            }
        }
        Ok(())
    }
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
    use super::sse_data;

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
}
