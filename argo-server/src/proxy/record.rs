//! What the proxy keeps of each exchange for the `model_exchange` table: who asked, for which
//! claim, when, what it cost, and the request (as a delta of its conversation) and reply.
//!
//! A conversation is one worker's requests for one claim (the `x-argo-chunk` and
//! `x-argo-generation` headers the worker sends). Each request repeats the whole conversation so
//! far, so only the messages after the longest prefix it shares with the conversation's
//! previous request are stored, with that prefix's length; [`rebuild`] reverses it.

use std::collections::HashMap;
use std::time::SystemTime;

use serde_json::Value;

use super::usage::Tokens;

/// One forwarded request and its outcome, ready to record.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    pub worker: String,
    pub claim: Claim,
    pub sent_at: SystemTime,
    pub answered_at: SystemTime,
    /// The provider's HTTP status, 0 if it wasn't reached.
    pub status: u16,
    pub tokens: Option<Tokens>,
    pub prefix: u32,
    pub request: String,
    pub response: String,
}

/// The claim a request names; both `None` for requests outside one (the startup tool check).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Claim {
    pub chunk: Option<u64>,
    pub generation: Option<u32>,
}

/// Each worker's current conversation: its claim and the full messages of its last request.
#[derive(Default)]
pub struct Conversations {
    last: HashMap<String, (Claim, Vec<Value>)>,
}

impl Conversations {
    /// `body` as stored: how many leading messages it shares with the same conversation's
    /// previous request, and the body with only the messages after those.
    pub fn delta(&mut self, worker: &str, claim: Claim, body: &Value) -> (u32, String) {
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let shared = match self.last.get(worker) {
            Some((last, previous)) if *last == claim && claim.chunk.is_some() => previous
                .iter()
                .zip(&messages)
                .take_while(|(a, b)| a == b)
                .count(),
            _ => 0,
        };
        let mut stored = body.clone();
        if let Some(object) = stored.as_object_mut()
            && object.contains_key("messages")
        {
            object.insert("messages".into(), Value::Array(messages[shared..].to_vec()));
        }
        self.last.insert(worker.to_string(), (claim, messages));
        (shared as u32, stored.to_string())
    }
}

/// The full request back from its stored form, given the previous request of its conversation
/// (already rebuilt): the format's definition, checked by the tests below.
#[cfg(test)]
pub fn rebuild(previous: Option<&Value>, prefix: u32, stored: &str) -> Result<Value, String> {
    let mut body: Value = serde_json::from_str(stored).map_err(|e| e.to_string())?;
    if prefix == 0 {
        return Ok(body);
    }
    let earlier = previous
        .and_then(|p| p.get("messages")?.as_array())
        .ok_or("a delta without its previous request")?;
    let shared = earlier
        .get(..prefix as usize)
        .ok_or("the previous request is shorter than the prefix")?;
    let tail = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("a delta without messages")?;
    let mut messages = shared.to_vec();
    messages.extend(tail.iter().cloned());
    body["messages"] = Value::Array(messages);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn claim(chunk: u64, generation: u32) -> Claim {
        Claim {
            chunk: Some(chunk),
            generation: Some(generation),
        }
    }

    #[test]
    fn a_conversation_stores_only_new_messages_and_rebuilds_exactly() {
        let mut c = Conversations::default();
        let system = json!({"role": "system", "content": "translate"});
        let first = json!({"model": "m", "tools": [1], "messages": [system, {"role": "user", "content": "f"}]});
        let (p1, s1) = c.delta("w1", claim(5, 1), &first);
        assert_eq!(p1, 0);
        assert_eq!(rebuild(None, p1, &s1).unwrap(), first);

        let mut second = first.clone();
        let more = second["messages"].as_array_mut().unwrap();
        more.push(json!({"role": "assistant", "content": "", "tool_calls": []}));
        more.push(json!({"role": "tool", "content": "rejected"}));
        let (p2, s2) = c.delta("w1", claim(5, 1), &second);
        assert_eq!(p2, 2, "system and user carried over");
        let stored: Value = serde_json::from_str(&s2).unwrap();
        assert_eq!(stored["messages"].as_array().unwrap().len(), 2);
        assert_eq!(stored["tools"], json!([1]), "everything else kept");
        assert_eq!(rebuild(Some(&first), p2, &s2).unwrap(), second);

        // An edited earlier message only shortens the shared prefix.
        let mut edited = second.clone();
        edited["messages"][1]["content"] = json!("g");
        let (p3, s3) = c.delta("w1", claim(5, 1), &edited);
        assert_eq!(p3, 1);
        assert_eq!(rebuild(Some(&second), p3, &s3).unwrap(), edited);
    }

    #[test]
    fn a_new_claim_or_none_starts_over() {
        let mut c = Conversations::default();
        let body = json!({"messages": [{"role": "user", "content": "x"}]});
        c.delta("w1", claim(5, 1), &body);
        assert_eq!(c.delta("w1", claim(6, 2), &body).0, 0, "next claim");
        assert_eq!(c.delta("w2", claim(6, 2), &body).0, 0, "another worker");
        assert_eq!(c.delta("w1", claim(6, 2), &body).0, 1);
        let none = Claim::default();
        c.delta("w1", none, &body);
        assert_eq!(
            c.delta("w1", none, &body).0,
            0,
            "unclaimed requests aren't joined"
        );
        assert!(rebuild(None, 1, r#"{"messages":[]}"#).is_err());
    }
}
