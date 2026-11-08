use super::Template;
use crate::{CNode, CNodeType};

/// Non-boundary type syntax. No resolved types, layouts, or pin types yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct TypeTemplate;

impl Template for TypeTemplate {
    fn id(&self) -> &'static str {
        "type"
    }

    fn matches(&self, node: &CNode) -> bool {
        matches!(
            node.node_type,
            CNodeType::Type | CNodeType::Field | CNodeType::Parameter | CNodeType::Enumerator
        )
    }
}
