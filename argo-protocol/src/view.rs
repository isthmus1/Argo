//! One hierarchy level as the GUI draws it (§5.5, §6): the children of a container, with any
//! opened child drawn in place as a frame around its own children.
//!
//! Frames carry no semantics. The protocol routes every value across a boundary through the
//! container's pins and its group input/output; when a child is opened in place those interface
//! nodes are hidden and each wire is resolved straight through them to the pins it really joins.
//! A closed container needs nothing special: its own pins are already its interface.

use std::collections::HashSet;

use crate::{Family, GraphIndex, NodeId, NodeKind, PinRef, Relation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    pub container: Option<NodeId>,
    pub nodes: Vec<LevelNode>,
    /// Opened children, outermost first.
    pub frames: Vec<Frame>,
    pub wires: Vec<LevelWire>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelNode {
    pub id: NodeId,
    /// The innermost frame it is drawn inside.
    pub frame: Option<NodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub id: NodeId,
    pub parent: Option<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LevelWire {
    pub from: PinRef,
    pub to: PinRef,
    pub family: Family,
}

/// Build the level for `container` (`None`: the roots). `open` says which descendants are opened in
/// place; only containers with children can open.
pub fn level(ix: &GraphIndex, container: Option<NodeId>, open: impl Fn(NodeId) -> bool) -> Level {
    let mut level = Level {
        container,
        nodes: Vec::new(),
        frames: Vec::new(),
        wires: Vec::new(),
    };
    let mut opened = HashSet::new();
    collect(ix, container, None, &open, &mut level, &mut opened);
    let visible: HashSet<NodeId> = level.nodes.iter().map(|n| n.id).collect();

    let mut seen = HashSet::new();
    for n in &level.nodes {
        for pin in ix.pins(n.id).filter(|p| p.direction.is_output()) {
            for e in ix.wires_from(&pin.pin_ref()) {
                let (family, to) = match &e.relation {
                    Relation::Exec { to, .. } => (Family::Control, to),
                    Relation::Data { to, .. } => (Family::Data, to),
                    _ => continue,
                };
                let mut sinks = Vec::new();
                resolve(ix, to, &visible, &opened, &mut sinks, 0);
                for to in sinks {
                    let wire = LevelWire {
                        from: pin.pin_ref(),
                        to,
                        family,
                    };
                    if seen.insert(wire.clone()) {
                        level.wires.push(wire);
                    }
                }
            }
        }
    }
    level
}

fn opens(ix: &GraphIndex, id: NodeId, open: &impl Fn(NodeId) -> bool) -> bool {
    ix.node(id).is_some_and(|n| n.kind.is_container())
        && !ix.children(Some(id)).is_empty()
        && open(id)
}

fn collect(
    ix: &GraphIndex,
    container: Option<NodeId>,
    frame: Option<NodeId>,
    open: &impl Fn(NodeId) -> bool,
    level: &mut Level,
    opened: &mut HashSet<NodeId>,
) {
    for &child in ix.children(container) {
        let Some(node) = ix.node(child) else { continue };
        if frame.is_some() && node.kind.is_interface() {
            continue; // hidden: wires resolve through it
        }
        if opens(ix, child, open) {
            opened.insert(child);
            level.frames.push(Frame {
                id: child,
                parent: frame,
            });
            collect(ix, Some(child), Some(child), open, level, opened);
        } else {
            level.nodes.push(LevelNode { id: child, frame });
        }
    }
}

/// Follow a wire's target through opened containers and hidden group outputs to visible pins.
fn resolve(
    ix: &GraphIndex,
    to: &PinRef,
    visible: &HashSet<NodeId>,
    opened: &HashSet<NodeId>,
    out: &mut Vec<PinRef>,
    depth: usize,
) {
    if depth > 64 {
        return;
    }
    let follow = |from: PinRef, out: &mut Vec<PinRef>| {
        for e in ix.wires_from(&from) {
            if let Some((_, next)) = e.relation.wire() {
                resolve(ix, next, visible, opened, out, depth + 1);
            }
        }
    };
    if visible.contains(&to.node) {
        out.push(to.clone());
    } else if opened.contains(&to.node) {
        // Into an opened container: continue from its group input's matching output.
        if let Some(gi) = ix.group_input(to.node) {
            follow(PinRef::new(gi, to.key.clone()), out);
        }
    } else if let Some(node) = ix.node(to.node) {
        // Into a hidden group output: continue from the opened container's matching output.
        if node.kind == NodeKind::GroupOutput
            && let Some(c) = node.parent.filter(|c| opened.contains(c))
        {
            follow(PinRef::new(c, to.key.clone()), out);
        }
    }
}
