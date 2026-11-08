use super::Template;
use crate::graph::{Expansion, Unsupported};
use crate::{CNode, CNodeType};

/// Prototype/extern declarations and statement declaration containers.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeclarationTemplate;

impl Template for DeclarationTemplate {
    fn id(&self) -> &'static str {
        "declaration"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Declaration
    }

    fn expand<'a>(&self, _node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        Ok(Expansion {
            node: None,
            children: Vec::new(),
        })
    }
}
