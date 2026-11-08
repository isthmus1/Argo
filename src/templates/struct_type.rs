use super::Template;
use crate::{CNode, CNodeType};

/// Struct definitions expose field syntax, not resolved layout or ABI pins.
#[derive(Debug, Default, Clone, Copy)]
pub struct StructTemplate;

impl Template for StructTemplate {
    fn id(&self) -> &'static str {
        "struct"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Struct
    }
}
