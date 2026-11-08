use super::Template;
use crate::{CNode, CNodeType};

/// Return is a syntax node here, not a standalone executable graph work unit.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReturnTemplate;

impl Template for ReturnTemplate {
    fn id(&self) -> &'static str {
        "return"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Return
    }
}
