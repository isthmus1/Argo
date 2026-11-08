//! Minimal typed effects (§5.6). They identify state to snapshot and dependencies to display; they
//! don't prove all behavior was captured. An unknown effect blocks a node from looking validated.

use serde::{Deserialize, Serialize};

use crate::{NodeId, pin::PinKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
    ReadWrite,
}

impl Access {
    pub fn reads(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }

    pub fn writes(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }
}

/// A node's effects. An empty list means "not analyzed yet", which is not the same as `Pure`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "effect")]
pub enum Effect {
    Pure,
    StateRead {
        state: NodeId,
        fields: Vec<String>,
    },
    StateWrite {
        state: NodeId,
        fields: Vec<String>,
    },
    /// Through the memory pin with this key.
    MemoryRead {
        pin: PinKey,
    },
    MemoryWrite {
        pin: PinKey,
    },
    /// Through the callback pin with this key.
    IndirectCall {
        pin: PinKey,
    },
    HostIo {
        resource: String,
    },
    /// Never returns (abort, exit, longjmp out).
    NonReturn,
    Unknown {
        reason: String,
    },
}

/// Summary of a node's effect list.
pub fn is_pure(effects: &[Effect]) -> bool {
    effects == [Effect::Pure]
}

pub fn is_unknown(effects: &[Effect]) -> bool {
    effects.is_empty() || effects.iter().any(|e| matches!(e, Effect::Unknown { .. }))
}
