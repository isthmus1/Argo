use super::Template;
use crate::{CNode, CNodeType};

/// Static assertions retain the expression, but do not evaluate it here.
#[derive(Debug, Default, Clone, Copy)]
pub struct StaticAssertTemplate;

impl Template for StaticAssertTemplate {
    fn id(&self) -> &'static str {
        "static_assert"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::StaticAssert
    }
}
