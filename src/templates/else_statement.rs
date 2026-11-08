use super::Template;
use crate::{CNode, CNodeType};

/// Else branches remain children of their owning if, including else-if chains.
#[derive(Debug, Default, Clone, Copy)]
pub struct ElseTemplate;

impl Template for ElseTemplate {
    fn id(&self) -> &'static str {
        "else"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Else
    }
}
