//! The worker's side of `argo-server`'s model proxy: attach for a key of its own, then send
//! completions there instead of to the provider. The key lives only in this process's memory,
//! never in its environment, so nothing the worker runs inherits it; and outside that proxy, or
//! once the server restarts, it is worth nothing.

use std::cell::RefCell;

/// One worker's attachment: where the proxy is, what this worker told it, and the key it got.
pub struct Proxy {
    url: String,
    label: serde_json::Value,
    /// The key and the worker name the proxy logs under, replaced on [`Proxy::reattach`].
    held: RefCell<(String, String)>,
}

impl Proxy {
    /// Attach as worker `job` of this host, connected to the database as `identity`.
    pub async fn attach(url: &str, job: u32, identity: &str) -> Result<Proxy, String> {
        let label = serde_json::json!({ "host": hostname(), "job": job, "identity": identity });
        let url = url.trim_end_matches('/').to_string();
        let held = RefCell::new(mint(&url, &label).await?);
        Ok(Proxy { url, label, held })
    }

    /// A fresh key, after the proxy refused the old one (the server restarted).
    pub async fn reattach(&self) -> Result<String, String> {
        let held = mint(&self.url, &self.label).await?;
        let name = held.1.clone();
        *self.held.borrow_mut() = held;
        Ok(name)
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn key(&self) -> String {
        self.held.borrow().0.clone()
    }

    /// The name the proxy's usage log knows this worker by.
    pub fn name(&self) -> String {
        self.held.borrow().1.clone()
    }
}

/// `POST /attach`: the key and worker name the proxy minted.
async fn mint(url: &str, label: &serde_json::Value) -> Result<(String, String), String> {
    let at = format!("{url}/attach");
    let fail = |why: String| {
        format!(
            "cannot attach to the model proxy at {url}: {why} (is argo-server running with OPENAI_API_KEY and OPENAI_BASE_URL set? Or pass --direct)"
        )
    };
    let reply = reqwest::Client::new()
        .post(&at)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(label.to_string())
        .send()
        .await
        .map_err(|e| fail(e.to_string()))?;
    let status = reply.status();
    let body = reply.bytes().await.map_err(|e| fail(e.to_string()))?;
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let field = |name| {
        json.get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    match (status.is_success(), field("key"), field("worker")) {
        (true, Some(key), Some(worker)) => Ok((key, worker)),
        _ => Err(fail(format!(
            "{status}: {}",
            String::from_utf8_lossy(&body)
        ))),
    }
}

/// Where `argo-server` puts its proxy by default: the database's host, at its port + 1.
pub fn beside(db: &str) -> Result<String, String> {
    let (scheme, rest) = if let Some(rest) = db.strip_prefix("ws://") {
        ("http", rest)
    } else if let Some(rest) = db.strip_prefix("wss://") {
        ("https", rest)
    } else {
        return Err(format!("--db {db}: expected ws:// or wss://"));
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let port = authority
        .rsplit_once(':')
        .filter(|(host, _)| !host.is_empty() && !authority.ends_with(']'))
        .and_then(|(host, port)| Some((host, port.parse::<u16>().ok()?.checked_add(1)?)));
    match port {
        Some((host, port)) => Ok(format!("{scheme}://{host}:{port}")),
        None => Err(format!(
            "--db {db} names no port to put the model proxy beside; pass --proxy URL"
        )),
    }
}

/// This machine's (or container's) name, for the proxy's log.
fn hostname() -> String {
    ["/proc/sys/kernel/hostname", "/etc/hostname"]
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::beside;

    #[test]
    fn the_proxy_sits_beside_the_database() {
        let at = |db| beside(db);
        assert_eq!(at("ws://10.0.0.5:3100"), Ok("http://10.0.0.5:3101".into()));
        assert_eq!(at("ws://desk:3100/"), Ok("http://desk:3101".into()));
        assert_eq!(
            at("wss://db.example.com:443"),
            Ok("https://db.example.com:444".into())
        );
        assert_eq!(at("ws://[::1]:3100"), Ok("http://[::1]:3101".into()));
        assert!(at("wss://db.example.com").is_err(), "no port");
        assert!(at("ws://[::1]").is_err(), "no port");
        assert!(at("http://desk:3100").is_err());
        assert!(at("ws://desk:65535").is_err());
    }
}
