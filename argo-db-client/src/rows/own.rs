//! Which chunk row carries each protocol record.

use std::collections::HashMap;

use argo_protocol::Graph;

use crate::partition::Partition;
use crate::records::Owned;

/// Each chunk's records, kept with their position in the graph.
pub fn own(graph: &Graph, parts: &Partition) -> HashMap<u64, Owned> {
    let mut owned = HashMap::<u64, Owned>::new();
    for (i, n) in (0..).zip(&graph.nodes) {
        let o = owned.entry(parts.owner(n.id)).or_default();
        o.nodes.push((i, n.clone()));
    }
    for (i, p) in (0..).zip(&graph.pins) {
        let o = owned.entry(parts.owner(p.node)).or_default();
        o.pins.push((i, p.clone()));
    }
    for (i, e) in (0..).zip(&graph.edges) {
        let o = owned.entry(parts.owner(e.relation.nodes().0)).or_default();
        o.edges.push((i, e.clone()));
    }
    for (i, l) in (0..).zip(&graph.layouts) {
        let o = owned.entry(parts.owner(l.node)).or_default();
        o.layouts.push((i, l.clone()));
    }
    owned
}
