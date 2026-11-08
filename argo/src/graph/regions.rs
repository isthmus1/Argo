use super::ChildRegion;
use crate::{CNode, CNodeType as Kind};

/// Group contiguous simple statements; controls and unsupported syntax are barriers.
pub(super) fn partition<'a>(nodes: impl IntoIterator<Item = &'a CNode>) -> Vec<ChildRegion<'a>> {
    let mut regions = Vec::new();
    let mut sequence = Vec::new();
    for node in nodes {
        if matches!(
            node.node_type,
            Kind::Declaration
                | Kind::Variable
                | Kind::Expression
                | Kind::Initializer
                | Kind::Return
                | Kind::Break
                | Kind::Continue
                | Kind::Empty
        ) {
            sequence.push(node);
        } else {
            if !sequence.is_empty() {
                regions.push(ChildRegion::Sequence(std::mem::take(&mut sequence)));
            }
            regions.push(ChildRegion::Syntax(node));
        }
    }
    if !sequence.is_empty() {
        regions.push(ChildRegion::Sequence(sequence));
    }
    regions
}
