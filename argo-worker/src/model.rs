//! The language model behind a trait, so the retry loop is tested with a scripted fake.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

/// Why a completion gave no usable reply. The split decides whether the chunk pays for it: a
/// misconfigured or unreachable endpoint must not exhaust every chunk's attempts and mark the
/// whole repository `Failed` (terminal) within seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    /// This prompt got no usable reply (rejected as too long or malformed, no text, too slow):
    /// counts as an attempt and is fed back.
    Reply(String),
    /// The endpoint itself cannot serve (unset configuration, connection failure, rate limit,
    /// server error): says nothing about the chunk, so it never uses up an attempt.
    Unavailable(String),
}

pub trait Model {
    /// One completion of `prompt` by `model`: the raw reply, or why there is none.
    fn complete(
        &self,
        model: &str,
        prompt: &str,
    ) -> impl Future<Output = Result<String, ModelError>>;
}

/// The environment iterator's client reads.
const ENV: [&str; 2] = ["OPENAI_API_KEY", "OPENAI_BASE_URL"];

/// Nemotron through iterator's OpenAI-compatible client (`OPENAI_API_KEY`, `OPENAI_BASE_URL`).
pub struct Nemotron {
    /// A hung request would hold the claim forever while the heartbeat keeps its lease alive.
    /// Running out counts as an attempt: it is bounded, so it cannot fail chunks in a burst.
    pub timeout: Duration,
}

impl Nemotron {
    /// Whether the endpoint is configured, checked before claiming anything.
    pub fn configured() -> Result<(), String> {
        match ENV.into_iter().find(|var| std::env::var(var).is_err()) {
            Some(var) => Err(format!("{var} is not set")),
            None => Ok(()),
        }
    }
}

impl Model for Nemotron {
    async fn complete(&self, model: &str, prompt: &str) -> Result<String, ModelError> {
        Nemotron::configured().map_err(ModelError::Unavailable)?;
        match tokio::time::timeout(self.timeout, iterator::nemo::complete(model, prompt)).await {
            Ok(reply) => reply.map_err(|e| classify(&*e)),
            Err(_) => Err(ModelError::Reply(format!(
                "model gave no reply within {:?}",
                self.timeout
            ))),
        }
    }
}

/// HTTP 400, 413 and 422 reject this request (too long, malformed for the model); every other
/// transport failure is the endpoint's. Anything not from HTTP is a reply without text.
fn classify(e: &(dyn Error + Send + Sync + 'static)) -> ModelError {
    let why = format!("model request failed: {e}");
    match e
        .downcast_ref::<reqwest::Error>()
        .map(reqwest::Error::status)
    {
        None => ModelError::Reply(why),
        Some(Some(s)) if matches!(s.as_u16(), 400 | 413 | 422) => ModelError::Reply(why),
        Some(_) => ModelError::Unavailable(why),
    }
}
