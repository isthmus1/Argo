use super::Template;
use crate::graph::{Expansion, GraphNodeKind, Unsupported};
use crate::{CNode, CNodeType};

/// A union is a distinct syntax construct, never inferred from a struct.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnionTemplate;

impl Template for UnionTemplate {
    fn id(&self) -> &'static str {
        "union"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Union
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        Ok(Expansion::leaf(GraphNodeKind::Type, node))
    }
}
