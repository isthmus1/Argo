use super::Template;
use crate::{CNode, CNodeType};

/// Case syntax includes its constant/range and optional labeled statement.
#[derive(Debug, Default, Clone, Copy)]
pub struct CaseTemplate;

impl Template for CaseTemplate {
    fn id(&self) -> &'static str {
        "case"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Case
    }
}
