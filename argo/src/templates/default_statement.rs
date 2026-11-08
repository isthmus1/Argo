use super::Template;
use crate::{CNode, CNodeType};

/// The default label is distinct from a numbered switch case.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultTemplate;

impl Template for DefaultTemplate {
    fn id(&self) -> &'static str {
        "default"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Default
    }
}
