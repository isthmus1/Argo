//! The real provider: OpenRouter (or `--model-url`), with `OPENROUTER_API_KEY` from this
//! process's environment, never sent anywhere but there. The server picks the model and, with
//! `--model-provider`, the one OpenRouter provider that serves it; every forwarded request is
//! rewritten to say so, whatever the worker sent.

use std::time::Duration;

use bytes::Bytes;
use serde_json::{Value, json};

/// Longer than the worker's own model timeout, so the worker's is the one that fires.
const TIMEOUT: Duration = Duration::from_secs(11 * 60);
/// The environment variable holding the provider key.
pub const KEY_VAR: &str = "OPENROUTER_API_KEY";

pub struct Upstream {
    base: String,
    key: String,
    model: String,
    /// An OpenRouter provider slug (`fireworks`) every request is pinned to, without fallbacks.
    provider: Option<String>,
    client: reqwest::Client,
}

impl Upstream {
    pub fn new(
        base: &str,
        key: &str,
        model: &str,
        provider: Option<&str>,
    ) -> Result<Upstream, String> {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| format!("cannot build the provider client: {e}"))?;
        Ok(Upstream {
            base: base.trim_end_matches('/').to_string(),
            key: key.to_string(),
            model: model.to_string(),
            provider: provider.map(str::to_string),
            client,
        })
    }

    /// The provider at `base` with the key from this server's environment, or why there is none.
    pub fn from_env(base: &str, model: &str, provider: Option<&str>) -> Result<Upstream, String> {
        let key = std::env::var(KEY_VAR).map_err(|_| format!("{KEY_VAR} is not set"))?;
        Upstream::new(base, &key, model, provider)
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Where requests go and as what, for the log.
    pub fn describe(&self) -> String {
        let via = self
            .provider
            .as_deref()
            .map_or("any provider".into(), |p| format!("provider {p} only"));
        format!("{} as {} via {via}", self.base, self.model)
    }

    /// The worker's request as the provider gets it: the server's model and provider pin, and
    /// OpenRouter's cost accounting in the reply's `usage`.
    pub fn rewrite(&self, body: &[u8]) -> Result<Bytes, String> {
        let mut json: Value =
            serde_json::from_slice(body).map_err(|e| format!("request body: {e}"))?;
        let request = json
            .as_object_mut()
            .ok_or("request body: not a JSON object")?;
        request.insert("model".into(), json!(self.model));
        request.insert("usage".into(), json!({ "include": true }));
        match &self.provider {
            Some(p) => request.insert(
                "provider".into(),
                json!({ "order": [p], "allow_fallbacks": false }),
            ),
            None => request.remove("provider"),
        };
        Ok(json.to_string().into())
    }

    /// Forward one chat-completion body: the provider's status and body, or why it couldn't be
    /// reached.
    pub async fn complete(&self, body: Bytes) -> Result<(u16, Bytes), String> {
        let reply = self
            .client
            .post(format!("{}/v1/chat/completions", self.base))
            .bearer_auth(&self.key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            // OpenRouter's app attribution.
            .header("X-Title", "Argo")
            .body(body)
            .send()
            .await
            .map_err(|e| format!("cannot reach the provider: {e}"))?;
        let status = reply.status().as_u16();
        let body = reply
            .bytes()
            .await
            .map_err(|e| format!("the provider's reply broke off: {e}"))?;
        Ok((status, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_take_the_servers_model_and_provider() {
        let pinned = Upstream::new("http://x/", "k", "nvidia/m", Some("fireworks")).unwrap();
        let body = br#"{"model":"other","provider":{"order":["cheap"]},"messages":[]}"#;
        let sent: Value = serde_json::from_slice(&pinned.rewrite(body).unwrap()).unwrap();
        assert_eq!(sent["model"], "nvidia/m");
        assert_eq!(
            sent["provider"],
            json!({"order": ["fireworks"], "allow_fallbacks": false})
        );
        assert_eq!(sent["usage"], json!({"include": true}));
        assert_eq!(sent["messages"], json!([]));
        assert_eq!(
            pinned.describe(),
            "http://x as nvidia/m via provider fireworks only"
        );

        let any = Upstream::new("http://x", "k", "nvidia/m:free", None).unwrap();
        let sent: Value = serde_json::from_slice(&any.rewrite(body).unwrap()).unwrap();
        assert!(
            sent.get("provider").is_none(),
            "no pin the server didn't ask for"
        );
        assert!(any.rewrite(b"[]").is_err() && any.rewrite(b"nope").is_err());
    }
}
