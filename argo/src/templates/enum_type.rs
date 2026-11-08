use super::Template;
use crate::graph::{Expansion, GraphNodeKind, Unsupported};
use crate::{CNode, CNodeType};

/// Enum definitions retain their named variants and initializer expressions.
#[derive(Debug, Default, Clone, Copy)]
pub struct EnumTemplate;

impl Template for EnumTemplate {
    fn id(&self) -> &'static str {
        "enum"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Enum
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        Ok(Expansion::leaf(GraphNodeKind::Type, node))
    }
}
