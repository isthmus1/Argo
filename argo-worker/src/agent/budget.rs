//! What a chunk may spend: rejected submissions, model turns, and the pauses through an outage.

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
    /// Every turn's model.
    pub model: String,
    /// The pause after the first outage, doubling with each consecutive one up to a minute.
    pub backoff: Duration,
}

impl Budget {
    pub(super) fn pause(&self, outages: u32) -> Duration {
        let doubled = self.backoff.saturating_mul(1u32 << (outages - 1).min(16));
        doubled.min(MAX_PAUSE)
    }
}
