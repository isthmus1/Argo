use super::Template;
use crate::{CNode, CNodeType};

/// An if condition, its then statement, and its optional else branch.
#[derive(Debug, Default, Clone, Copy)]
pub struct IfTemplate;

impl Template for IfTemplate {
    fn id(&self) -> &'static str {
        "if"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::If
    }
}
