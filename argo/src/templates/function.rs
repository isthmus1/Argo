use super::Template;
use crate::graph::{Expansion, Unsupported};
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

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        crate::graph::constructs::function(node)
    }
}
