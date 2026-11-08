use super::Template;
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
}
