//! The keys the proxy mints: one per attached worker, valid only against this proxy and only
//! until the server stops (they live in memory). The ledger of what each worker has spent rides
//! along with its key.

use std::collections::HashMap;
use std::fmt::Write;

use super::usage::{Tokens, Usage};

/// One attached worker: what it says it is, and what it has spent.
pub struct Worker {
    pub id: u32,
    pub label: String,
    pub usage: Usage,
}

impl Worker {
    pub fn name(&self) -> String {
        format!("w{}", self.id)
    }
}

/// Every key minted since the server started, and the sum of their usage.
#[derive(Default)]
pub struct Workers {
    by_key: HashMap<String, Worker>,
    attached: u32,
    pub total: Usage,
}

impl Workers {
    /// A new worker under a fresh key: the key (which only this call returns) and the worker.
    pub fn attach(&mut self, label: String) -> Result<(String, &Worker), String> {
        let key = mint()?;
        self.attached += 1;
        let usage = Usage::default();
        let id = self.attached;
        let worker = self
            .by_key
            .entry(key.clone())
            .insert_entry(Worker { id, label, usage })
            .into_mut();
        Ok((key, worker))
    }

    pub fn get(&self, key: &str) -> Option<&Worker> {
        self.by_key.get(key)
    }

    /// Count one forwarded request against `key`'s worker and the total.
    pub fn record(&mut self, key: &str, ok: bool, tokens: Option<Tokens>) -> Option<&Worker> {
        let worker = self.by_key.get_mut(key)?;
        worker.usage.add(ok, tokens);
        self.total.add(ok, tokens);
        Some(worker)
    }
}

/// 256 random bits from the OS, hex: unguessable, and recognisable as Argo's in a log or config.
fn mint() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("cannot mint a worker key: {e}"))?;
    let mut key = String::from("argo-");
    for b in bytes {
        let _ = write!(key, "{b:02x}");
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_worker_gets_its_own_key_and_ledger() {
        let mut ws = Workers::default();
        let (a, wa) = ws.attach("host=a".into()).unwrap();
        assert_eq!((wa.name(), wa.label.as_str()), ("w1".into(), "host=a"));
        let (b, _) = ws.attach("host=b".into()).unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("argo-") && a.len() == 5 + 64, "{a}");

        let t = Tokens {
            prompt: 10,
            completion: 5,
            total: 15,
        };
        assert_eq!(ws.record(&a, true, Some(t)).unwrap().usage.requests, 1);
        ws.record(&b, false, None);
        assert!(ws.record("argo-unknown", true, Some(t)).is_none());
        assert_eq!(ws.get(&a).unwrap().usage.tokens, t);
        assert_eq!(ws.get(&b).unwrap().usage.failed, 1);
        assert_eq!((ws.total.requests, ws.total.failed), (2, 1));
        assert_eq!(ws.total.tokens, t);
    }
}
