use super::Template;
use crate::graph::{Expansion, GraphNodeKind, Unsupported};
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

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        Ok(Expansion::leaf(GraphNodeKind::State, node))
    }
}
