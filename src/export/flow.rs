//! Control exits and fallthrough, derived from lexically present `return`/`break`/`continue`.

use std::collections::BTreeSet;

use argo_protocol::{Flow, pin::key};

use super::Exporter;
use crate::graph::{ControlExitKind, GraphNodeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Exit {
    Return,
    Break,
    Continue,
}

impl Exit {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Return => key::RETURN_EXIT,
            Self::Break => key::BREAK,
            Self::Continue => key::CONTINUE,
        }
    }

    pub(super) fn flow(self) -> Flow {
        match self {
            Self::Return => Flow::Return,
            Self::Break => Flow::Break,
            Self::Continue => Flow::Continue,
        }
    }
}

impl Exporter<'_> {
    /// Exits that leave this node: loops capture `break`/`continue`, switches capture `break`,
    /// and a function's `return` is its normal completion.
    pub(super) fn escaping(&self, id: usize) -> BTreeSet<Exit> {
        let n = &self.g.nodes[id];
        let mut exits: BTreeSet<Exit> = n
            .exits
            .iter()
            .map(|e| match e.kind {
                ControlExitKind::Return => Exit::Return,
                ControlExitKind::Break => Exit::Break,
                ControlExitKind::Continue => Exit::Continue,
            })
            .collect();
        for &c in &n.children {
            exits.extend(self.escaping(c));
        }
        match n.kind {
            GraphNodeKind::Loop => {
                exits.remove(&Exit::Break);
                exits.remove(&Exit::Continue);
            }
            GraphNodeKind::Switch => {
                exits.remove(&Exit::Break);
            }
            GraphNodeKind::Function => exits.clear(),
            _ => {}
        }
        exits
    }

    /// Whether control can fall off the end. Conservative: only an unconditional exit as the last
    /// statement, or every arm exiting, proves it can't.
    pub(super) fn falls_through(&self, id: usize) -> bool {
        let n = &self.g.nodes[id];
        match n.kind {
            GraphNodeKind::Sequence => n.exits.last().is_none_or(|e| e.span.hi < n.span.hi),
            GraphNodeKind::Scope | GraphNodeKind::CaseGroup => {
                n.children.last().is_none_or(|&c| self.falls_through(c))
            }
            GraphNodeKind::Branch => {
                n.children.len() < 2 || n.children.iter().any(|&c| self.falls_through(c))
            }
            _ => true,
        }
    }
}

/// Types and globals are data-only and never join the exec chain.
pub(super) fn is_exec(kind: GraphNodeKind) -> bool {
    !matches!(kind, GraphNodeKind::Type | GraphNodeKind::State)
}
