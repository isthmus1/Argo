//! Token usage, as the provider reports it in each completion's `usage`, and the usage log: one
//! line per attach and per forwarded request, each with that worker's and the overall totals so
//! far, so the latest line for a worker is its current tally.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tokens {
    pub prompt: u64,
    pub completion: u64,
    pub total: u64,
}

impl Tokens {
    /// The `usage` object of an OpenAI-compatible completion; `None` if the body has none.
    pub fn of(body: &[u8]) -> Option<Tokens> {
        let json: serde_json::Value = serde_json::from_slice(body).ok()?;
        let usage = json.get("usage")?;
        let field = |name| usage.get(name).and_then(serde_json::Value::as_u64);
        let (prompt, completion) = (field("prompt_tokens")?, field("completion_tokens")?);
        let total = field("total_tokens").unwrap_or(prompt + completion);
        Some(Tokens {
            prompt,
            completion,
            total,
        })
    }

    fn add(&mut self, other: Tokens) {
        self.prompt += other.prompt;
        self.completion += other.completion;
        self.total += other.total;
    }
}

impl fmt::Display for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}+{}={}", self.prompt, self.completion, self.total)
    }
}

/// Requests forwarded, how many the provider didn't answer with success, and the tokens spent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub requests: u64,
    pub failed: u64,
    pub tokens: Tokens,
}

impl Usage {
    pub fn add(&mut self, ok: bool, tokens: Option<Tokens>) {
        self.requests += 1;
        self.failed += u64::from(!ok);
        if let Some(t) = tokens {
            self.tokens.add(t);
        }
    }
}

impl fmt::Display for Usage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} requests ({} failed), tokens {}",
            self.requests, self.failed, self.tokens
        )
    }
}

/// The append-only usage log. A write that fails is reported, never fatal: the proxy keeps
/// serving workers whether or not the disk keeps up.
pub struct Log {
    file: File,
}

impl Log {
    pub fn open(path: &Path) -> Result<Log, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Log { file })
    }

    pub fn line(&mut self, text: &str) {
        let at = humantime::format_rfc3339_seconds(SystemTime::now());
        if let Err(e) = writeln!(self.file, "{at} {text}") {
            eprintln!("argo-server: cannot write the usage log: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_comes_from_the_completion_body() {
        let body = br#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":30,"total_tokens":42}}"#;
        let t = Tokens::of(body).unwrap();
        assert_eq!(t.to_string(), "12+30=42");
        let no_total = br#"{"usage":{"prompt_tokens":2,"completion_tokens":3}}"#;
        assert_eq!(Tokens::of(no_total).unwrap().total, 5);
        assert_eq!(Tokens::of(br#"{"choices":[]}"#), None);
        assert_eq!(Tokens::of(b"not json"), None);
    }

    #[test]
    fn totals_count_failures_and_tokens() {
        let mut u = Usage::default();
        u.add(
            true,
            Tokens::of(br#"{"usage":{"prompt_tokens":2,"completion_tokens":3}}"#),
        );
        u.add(false, None);
        assert_eq!(u.to_string(), "2 requests (1 failed), tokens 2+3=5");
    }
}
