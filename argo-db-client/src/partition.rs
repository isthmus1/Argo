//! Which chunk owns each node, and the shape of the chunk tree.
//!
//! Chunk heads are the modules, every work unit, and every top-level item (types, globals,
//! functions); the program node belongs to the project row. Any other node belongs to its
//! nearest enclosing head, so a function's interior travels with the function.

mod shape;

use std::collections::HashMap;

use argo_protocol::{Graph, Node, NodeId, NodeKind};

pub use shape::Head;

pub struct Partition<'g> {
    /// In graph node order.
    pub heads: Vec<Head<'g>>,
    owner: HashMap<NodeId, u64>,
    root: u64,
}

impl Partition<'_> {
    /// The chunk owning `node`; records of unknown nodes go to the project row.
    pub fn owner(&self, node: NodeId) -> u64 {
        self.owner.get(&node).copied().unwrap_or(self.root)
    }
}

/// Parent lookups over one graph.
pub struct Tree<'g> {
    by_id: HashMap<NodeId, &'g Node>,
}

impl<'g> Tree<'g> {
    pub fn parent(&self, n: &Node) -> Option<&'g Node> {
        n.parent.and_then(|p| self.by_id.get(&p).copied())
    }

    /// `n` and its ancestors, innermost first. Bounded, so a malformed cycle can't hang.
    pub fn up(&self, n: &'g Node) -> impl Iterator<Item = &'g Node> + '_ {
        std::iter::successors(Some(n), |a| self.parent(a)).take(self.by_id.len() + 1)
    }
}

/// Heads get ids `first, first + 1, …` in node order; `root` is the project row's id.
pub fn partition(graph: &Graph, root: u64, first: u64) -> Partition<'_> {
    let tree = Tree {
        by_id: graph.nodes.iter().map(|n| (n.id, n)).collect(),
    };
    let program = graph
        .nodes
        .iter()
        .find(|n| n.parent.is_none() && n.kind == NodeKind::Program)
        .map(|n| n.id);
    let top =
        |p: Option<&Node>| p.is_none_or(|p| matches!(p.kind, NodeKind::Program | NodeKind::Module));
    let heads: Vec<&Node> = graph
        .nodes
        .iter()
        .filter(|n| Some(n.id) != program)
        .filter(|n| n.work_unit || n.kind == NodeKind::Module || top(tree.parent(n)))
        .collect();
    let mut owner: HashMap<NodeId, u64> = heads
        .iter()
        .zip(first..)
        .map(|(n, id)| (n.id, id))
        .collect();
    for n in &graph.nodes {
        let chunk = tree
            .up(n)
            .find_map(|a| owner.get(&a.id).copied())
            .unwrap_or(root);
        owner.entry(n.id).or_insert(chunk);
    }
    let heads = shape::heads(&tree, &heads, &owner, root);
    Partition { heads, owner, root }
}
