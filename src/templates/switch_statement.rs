use super::Template;
use crate::{CNode, CNodeType};

/// Preserve the switch/case tree; safe fallthrough grouping is a later pass.
#[derive(Debug, Default, Clone, Copy)]
pub struct SwitchTemplate;

impl Template for SwitchTemplate {
    fn id(&self) -> &'static str {
        "switch"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Switch
    }
}
