//! Protocol-neutral request/response vocabulary shared by every chat backend.

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

/// A message's speaker. Each backend maps these onto its own wire roles
/// (OpenAI `system/assistant/user`, Anthropic's top-level `system` + content
/// blocks, Gemini `user/model` + `systemInstruction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    Assistant,
    User,
}

impl From<&str> for Role {
    fn from(role: &str) -> Self {
        match role {
            "system" => Role::System,
            "assistant" => Role::Assistant,
            _ => Role::User,
        }
    }
}

/// One message in a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

/// Everything a backend needs to run one completion.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub model: String,
    pub temperature: f32,
}
