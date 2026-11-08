use super::Template;
use crate::CNode;

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
}
