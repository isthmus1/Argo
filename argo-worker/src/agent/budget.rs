//! What a chunk may spend: rejected submissions, model turns, which model each turn uses, and
//! the pauses through an outage.

use std::time::Duration;

/// Consecutive outages waited out before giving up: with a 1 s backoff, about three minutes.
pub(super) const OUTAGES: u32 = 8;
/// The longest pause between outages.
const MAX_PAUSE: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct Budget {
    /// Rejected submissions (and failed requests) before the chunk fails; at least 1.
    pub max_attempts: u32,
    /// Model turns before the chunk fails, however they were spent; at least 1.
    pub max_turns: u32,
    pub model: String,
    pub escalation: Option<Escalation>,
    /// The pause after the first outage, doubling with each consecutive one up to a minute.
    pub backoff: Duration,
}

#[derive(Clone)]
pub struct Escalation {
    pub model: String,
    /// Rejected submissions (and failed requests) before switching.
    pub after: u32,
}

impl Budget {
    pub(super) fn model(&self, failures: u32) -> &str {
        match &self.escalation {
            Some(e) if failures >= e.after => &e.model,
            _ => &self.model,
        }
    }

    pub(super) fn pause(&self, outages: u32) -> Duration {
        let doubled = self.backoff.saturating_mul(1u32 << (outages - 1).min(16));
        doubled.min(MAX_PAUSE)
    }
}
