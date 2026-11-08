use super::Template;
use crate::{CNode, CNodeType};

/// Local/global object syntax; storage effects require later semantic analysis.
#[derive(Debug, Default, Clone, Copy)]
pub struct VariableTemplate;

impl Template for VariableTemplate {
    fn id(&self) -> &'static str {
        "variable"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Variable
    }
}
