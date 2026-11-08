//! Per-head metadata the module schedules by: kind, parent chunk, depth and translatable
//! descendants (leaves first, deepest first).

use std::collections::HashMap;

use argo_protocol::{Node, NodeId, NodeKind};

use super::Tree;
use crate::ChunkKind;

pub struct Head<'g> {
    pub node: &'g Node,
    pub id: u64,
    pub parent: u64,
    pub depth: u32,
    pub descendants: u32,
    pub kind: ChunkKind,
    /// The enclosing module's name: the original file, relative to the repository root.
    pub path: String,
}

fn kind(n: &Node) -> ChunkKind {
    match n.kind {
        NodeKind::Module => ChunkKind::Module,
        NodeKind::Function => ChunkKind::Function,
        NodeKind::Type(_) => ChunkKind::Type,
        NodeKind::State => ChunkKind::State,
        _ if n.work_unit => ChunkKind::Region,
        _ => ChunkKind::Other,
    }
}

pub fn heads<'g>(
    tree: &Tree<'g>,
    nodes: &[&'g Node],
    owner: &HashMap<NodeId, u64>,
    root: u64,
) -> Vec<Head<'g>> {
    let parent_of = |n: &Node| tree.parent(n).map_or(root, |p| owner[&p.id]);
    let parents: HashMap<u64, u64> = nodes.iter().map(|n| (owner[&n.id], parent_of(n))).collect();
    // Chunk ancestors, innermost first, ending at the project row.
    let chain = |id: u64| {
        std::iter::successors(parents.get(&id).copied(), |p| parents.get(p).copied())
            .take(parents.len() + 1)
    };
    let mut descendants: HashMap<u64, u32> = HashMap::new();
    for n in nodes.iter().filter(|n| n.work_unit) {
        for a in chain(owner[&n.id]) {
            *descendants.entry(a).or_default() += 1;
        }
    }
    nodes
        .iter()
        .map(|&n| {
            let id = owner[&n.id];
            Head {
                node: n,
                id,
                parent: parent_of(n),
                depth: chain(id).count() as u32,
                descendants: descendants.get(&id).copied().unwrap_or(0),
                kind: kind(n),
                path: tree
                    .up(n)
                    .find(|a| a.kind == NodeKind::Module)
                    .map_or_else(String::new, |m| m.name.clone()),
            }
        })
        .collect()
}
