use super::Template;
use crate::{CNode, CNodeType};

/// A do/while loop retains body-before-condition syntax.
#[derive(Debug, Default, Clone, Copy)]
pub struct DoWhileTemplate;

impl Template for DoWhileTemplate {
    fn id(&self) -> &'static str {
        "do"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::DoWhile
    }
}
