use super::Template;
use crate::graph::{Expansion, Unsupported};
use crate::{CNode, CNodeType};

/// Break remains syntax; its target/control-exit analysis is a later pass.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreakTemplate;

impl Template for BreakTemplate {
    fn id(&self) -> &'static str {
        "break"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Break
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        crate::graph::constructs::straight_line(node)
    }
}
