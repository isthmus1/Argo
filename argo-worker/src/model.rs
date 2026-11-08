//! The language model behind a trait, so the retry loop is tested with a scripted fake.

use std::error::Error;
use std::future::Future;
use std::time::Duration;

use crate::proxy::Proxy;

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

/// The OpenRouter key `--direct` mode reads.
const KEY_VAR: &str = "OPENROUTER_API_KEY";

/// Nemotron through iterator's OpenAI-compatible client.
pub struct Nemotron {
    /// A hung request would hold the claim forever while the heartbeat keeps its lease alive.
    /// Running out counts as an attempt: it is bounded, so it cannot fail chunks in a burst.
    pub timeout: Duration,
    pub access: Access,
}

/// How the worker reaches the model.
pub enum Access {
    /// OpenRouter itself, with `OPENROUTER_API_KEY` from the environment.
    Direct,
    /// `argo-server`'s model proxy, with the key it minted for this worker.
    Proxy(Proxy),
}

impl Nemotron {
    /// Whether the direct endpoint is configured, checked before claiming anything.
    pub fn configured() -> Result<String, String> {
        std::env::var(KEY_VAR).map_err(|_| format!("{KEY_VAR} is not set"))
    }

    async fn ask(&self, model: &str, prompt: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        match &self.access {
            Access::Direct => {
                let key = Nemotron::configured()?;
                iterator::nemo::complete_at(iterator::nemo::OPENROUTER, &key, model, prompt).await
            }
            Access::Proxy(p) => iterator::nemo::complete_at(p.url(), &p.key(), model, prompt).await,
        }
    }
}

impl Model for Nemotron {
    async fn complete(&self, model: &str, prompt: &str) -> Result<String, ModelError> {
        if let Access::Direct = self.access {
            Nemotron::configured().map_err(ModelError::Unavailable)?;
        }
        let e = match tokio::time::timeout(self.timeout, self.ask(model, prompt)).await {
            Ok(Ok(reply)) => return Ok(reply),
            Ok(Err(e)) => e,
            Err(_) => {
                return Err(ModelError::Reply(format!(
                    "model gave no reply within {:?}",
                    self.timeout
                )));
            }
        };
        match (&self.access, status(&*e)) {
            // The proxy forgot this worker's key (the server restarted): attach again, then let
            // the outage backoff retry with the new key.
            (Access::Proxy(p), Some(401)) => {
                Err(ModelError::Unavailable(match p.reattach().await {
                    Ok(name) => format!(
                        "the model proxy refused this worker's key; attached again as {name}"
                    ),
                    Err(why) => format!("the model proxy refused this worker's key; {why}"),
                }))
            }
            _ => Err(classify(&*e)),
        }
    }
}

/// The HTTP status of a failed request, if it got that far.
fn status(e: &(dyn Error + Send + Sync + 'static)) -> Option<u16> {
    let status = e.downcast_ref::<reqwest::Error>()?.status()?;
    Some(status.as_u16())
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

#[cfg(test)]
mod tests;
