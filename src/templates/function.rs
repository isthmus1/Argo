use super::Template;
use crate::{CNode, CNodeType};

/// Function definitions, not prototypes or function-pointer objects.
#[derive(Debug, Default, Clone, Copy)]
pub struct FunctionTemplate;

impl Template for FunctionTemplate {
    fn id(&self) -> &'static str {
        "function"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Function
    }
}
