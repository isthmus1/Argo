//! The language model behind a trait, so the agent loop is tested with a scripted fake: one chat
//! turn with tools, in the OpenAI chat-completions shape that OpenRouter and `argo-server`'s model
//! proxy speak.

use std::future::Future;
use std::time::Duration;

use serde_json::{Value, json};

use crate::proxy::Proxy;

mod wire;

/// Why a turn gave no usable reply. The split decides whether the chunk pays for it: a
/// misconfigured or unreachable endpoint must not exhaust every chunk's attempts and mark the
/// whole repository `Failed` (terminal) within seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    /// This request got no usable reply (rejected as too long or malformed, too slow): counts
    /// against the chunk.
    Reply(String),
    /// The endpoint itself cannot serve (unset configuration, connection failure, rate limit,
    /// server error): says nothing about the chunk, so it never uses up an attempt.
    Unavailable(String),
}

/// One message of the conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    System(String),
    User(String),
    Assistant(Reply),
    /// The result of the assistant's tool call `call`.
    Tool {
        call: String,
        content: String,
    },
}

/// What the model said in one turn: text, tool calls, or both.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub content: Option<String>,
    pub calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The arguments as the model wrote them: a JSON object, not yet parsed.
    pub arguments: String,
}

/// A tool the model may call: its name, what it does, and its arguments' JSON Schema.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

impl ToolSpec {
    fn wire(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }
}

pub trait Model {
    /// One turn of `model` on `messages`, with `tools` available.
    fn chat(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolSpec],
    ) -> impl Future<Output = Result<Reply, ModelError>>;
}

/// The OpenRouter key `--direct` mode reads.
const KEY_VAR: &str = "OPENROUTER_API_KEY";
/// OpenRouter's OpenAI-compatible API, which `--direct` mode sends to (`/v1/chat/completions`
/// is appended); `argo-server --model-url` defaults to the same.
const OPENROUTER: &str = "https://openrouter.ai/api";

/// Nemotron behind an OpenAI-compatible endpoint: OpenRouter, or `argo-server`'s model proxy.
pub struct Nemotron {
    /// A hung request would hold the claim forever while the heartbeat keeps its lease alive.
    /// Running out counts against the chunk: it is bounded, so it cannot fail chunks in a burst.
    pub timeout: Duration,
    pub access: Access,
    client: reqwest::Client,
}

/// How the worker reaches the model.
pub enum Access {
    /// OpenRouter itself, with `OPENROUTER_API_KEY` from the environment.
    Direct,
    /// `argo-server`'s model proxy, with the key it minted for this worker.
    Proxy(Proxy),
}

impl Nemotron {
    pub fn new(timeout: Duration, access: Access) -> Nemotron {
        Nemotron {
            timeout,
            access,
            client: reqwest::Client::new(),
        }
    }

    /// Whether the direct endpoint is configured, checked before claiming anything.
    pub fn configured() -> Result<String, String> {
        std::env::var(KEY_VAR).map_err(|_| format!("{KEY_VAR} is not set"))
    }

    /// Whether `model` accepts a request offering a tool (it needn't call it): a server
    /// without tool calling rejects it outright.
    pub async fn supports_tools(&self, model: &str) -> Result<(), ModelError> {
        let ping = ToolSpec {
            name: "ping",
            description: "Answer the check.",
            parameters: json!({"type": "object", "properties": {}, "additionalProperties": false}),
        };
        let ask = [Message::User("Call the ping tool.".into())];
        self.chat(model, &ask, &[ping]).await.map(drop)
    }

    /// Where requests go, and the key they carry.
    fn endpoint(&self) -> Result<(String, String), ModelError> {
        match &self.access {
            Access::Direct => {
                let key = Nemotron::configured().map_err(ModelError::Unavailable)?;
                Ok((OPENROUTER.to_string(), key))
            }
            Access::Proxy(p) => Ok((p.url().to_string(), p.key())),
        }
    }
}

impl Model for Nemotron {
    async fn chat(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolSpec],
    ) -> Result<Reply, ModelError> {
        let (base, key) = self.endpoint()?;
        let url = format!("{}/v1/chat/completions", base.trim_end_matches('/'));
        let body = wire::request(model, messages, tools);
        let send = self.client.post(url).bearer_auth(key).json(&body).send();
        let response = match tokio::time::timeout(self.timeout, send).await {
            Err(_) => {
                return Err(ModelError::Reply(format!(
                    "model gave no reply within {:?}",
                    self.timeout
                )));
            }
            Ok(Err(e)) => {
                return Err(ModelError::Unavailable(format!(
                    "model request failed: {e}"
                )));
            }
            Ok(Ok(r)) => r,
        };
        let status = response.status().as_u16();
        let text = match tokio::time::timeout(self.timeout, response.text()).await {
            Ok(Ok(text)) => text,
            Ok(Err(e)) => {
                return Err(ModelError::Unavailable(format!("reading the reply: {e}")));
            }
            Err(_) => return Err(ModelError::Reply("the reply stalled".into())),
        };
        match (&self.access, status) {
            (_, 200..=299) => wire::reply(&text).map_err(ModelError::Reply),
            // The proxy forgot this worker's key (the server restarted): attach again, then let
            // the outage backoff retry with the new key.
            (Access::Proxy(p), 401) => Err(ModelError::Unavailable(match p.reattach().await {
                Ok(name) => {
                    format!("the model proxy refused this worker's key; attached again as {name}")
                }
                Err(why) => format!("the model proxy refused this worker's key; {why}"),
            })),
            _ => Err(classify(status, &text)),
        }
    }
}

/// HTTP 400, 413 and 422 reject this request (too long, malformed for the model), as does
/// OpenRouter's 404 for a model or pinned provider without tool calling, which waiting won't fix;
/// every other failure is the endpoint's.
fn classify(status: u16, body: &str) -> ModelError {
    let why = format!(
        "model request failed: HTTP {status}: {}",
        crate::tools::cut(body.trim().to_string(), 2000)
    );
    match status {
        400 | 413 | 422 => ModelError::Reply(why),
        404 if body.to_ascii_lowercase().contains("tool") => ModelError::Reply(why),
        _ => ModelError::Unavailable(why),
    }
}

#[cfg(test)]
mod tests;
