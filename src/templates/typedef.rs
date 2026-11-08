use super::Template;
use crate::{CNode, CNodeType};

/// Typedefs declare type aliases, not variable/state nodes.
#[derive(Debug, Default, Clone, Copy)]
pub struct TypedefTemplate;

impl Template for TypedefTemplate {
    fn id(&self) -> &'static str {
        "typedef"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Typedef
    }
}
