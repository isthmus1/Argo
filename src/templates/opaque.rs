use super::Template;
use crate::CNode;
use crate::graph::{Expansion, Unsupported};

/// Catch-all recognizer. Keep this last so unknown syntax is never discarded.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpaqueTemplate;

impl Template for OpaqueTemplate {
    fn id(&self) -> &'static str {
        "opaque"
    }

    fn matches(&self, _node: &CNode) -> bool {
        true
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        Ok(Expansion::opaque(
            node,
            "no graph expansion supports this construct",
        ))
    }
}
