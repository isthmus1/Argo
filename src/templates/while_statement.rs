use super::Template;
use crate::{CNode, CNodeType};

/// While syntax preserves pre-test condition/body order.
#[derive(Debug, Default, Clone, Copy)]
pub struct WhileTemplate;

impl Template for WhileTemplate {
    fn id(&self) -> &'static str {
        "while"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::While
    }
}
