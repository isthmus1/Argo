use super::Template;
use crate::{CNode, CNodeType};

/// A compound statement owns the original ordered statements in its scope.
#[derive(Debug, Default, Clone, Copy)]
pub struct BlockTemplate;

impl Template for BlockTemplate {
    fn id(&self) -> &'static str {
        "block"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Block
    }
}
