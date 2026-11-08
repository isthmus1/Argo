//! The real provider: `OPENAI_BASE_URL` with `OPENAI_API_KEY`, read from this process's
//! environment and never sent anywhere but there.

use std::time::Duration;

use bytes::Bytes;

/// Longer than the worker's own model timeout, so the worker's is the one that fires.
const TIMEOUT: Duration = Duration::from_secs(11 * 60);

pub struct Upstream {
    base: String,
    key: String,
    client: reqwest::Client,
}

impl Upstream {
    pub fn new(base: &str, key: &str) -> Result<Upstream, String> {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| format!("cannot build the provider client: {e}"))?;
        let base = base.trim_end_matches('/').to_string();
        let key = key.to_string();
        Ok(Upstream { base, key, client })
    }

    /// The provider this server's environment names, or which variable is missing.
    pub fn from_env() -> Result<Upstream, String> {
        let var = |name| std::env::var(name).map_err(|_| format!("{name} is not set"));
        Upstream::new(&var("OPENAI_BASE_URL")?, &var("OPENAI_API_KEY")?)
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Forward one chat-completion body as it came: the provider's status and body, or why it
    /// couldn't be reached.
    pub async fn complete(&self, body: Bytes) -> Result<(u16, Bytes), String> {
        let reply = self
            .client
            .post(format!("{}/v1/chat/completions", self.base))
            .bearer_auth(&self.key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
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
