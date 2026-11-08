//! Lookup tables over one graph revision: children in order, pins by node, wires by pin.

use std::collections::HashMap;

use crate::{Edge, Graph, Node, NodeId, NodeKind, Pin, PinRef};

pub struct GraphIndex<'g> {
    pub graph: &'g Graph,
    nodes: HashMap<NodeId, usize>,
    children: HashMap<Option<NodeId>, Vec<NodeId>>,
    pins: HashMap<NodeId, Vec<usize>>,
    pin_at: HashMap<PinRef, usize>,
    wires_from: HashMap<PinRef, Vec<usize>>,
    wires_to: HashMap<PinRef, Vec<usize>>,
}

impl<'g> GraphIndex<'g> {
    pub fn new(graph: &'g Graph) -> Self {
        let mut ix = Self {
            graph,
            nodes: HashMap::new(),
            children: HashMap::new(),
            pins: HashMap::new(),
            pin_at: HashMap::new(),
            wires_from: HashMap::new(),
            wires_to: HashMap::new(),
        };
        for (i, n) in graph.nodes.iter().enumerate() {
            ix.nodes.entry(n.id).or_insert(i);
            ix.children.entry(n.parent).or_default().push(n.id);
        }
        for kids in ix.children.values_mut() {
            kids.sort_by_key(|id| (graph.nodes[ix.nodes[id]].order, *id));
        }
        for (i, p) in graph.pins.iter().enumerate() {
            ix.pins.entry(p.node).or_default().push(i);
            ix.pin_at.entry(p.pin_ref()).or_insert(i);
        }
        for (i, e) in graph.edges.iter().enumerate() {
            if let Some((from, to)) = e.relation.wire() {
                ix.wires_from.entry(from.clone()).or_default().push(i);
                ix.wires_to.entry(to.clone()).or_default().push(i);
            }
        }
        ix
    }

    pub fn node(&self, id: NodeId) -> Option<&'g Node> {
        self.nodes.get(&id).map(|&i| &self.graph.nodes[i])
    }

    /// Children in sibling order; `None` gives the roots.
    pub fn children(&self, parent: Option<NodeId>) -> &[NodeId] {
        self.children.get(&parent).map_or(&[], Vec::as_slice)
    }

    pub fn roots(&self) -> &[NodeId] {
        self.children(None)
    }

    pub fn pins(&self, node: NodeId) -> impl Iterator<Item = &'g Pin> + '_ {
        let graph = self.graph;
        self.pins
            .get(&node)
            .into_iter()
            .flatten()
            .map(move |&i| &graph.pins[i])
    }

    pub fn pin(&self, pin: &PinRef) -> Option<&'g Pin> {
        self.pin_at.get(pin).map(|&i| &self.graph.pins[i])
    }

    pub fn wires_from(&self, pin: &PinRef) -> impl Iterator<Item = &'g Edge> + '_ {
        let graph = self.graph;
        self.wires_from
            .get(pin)
            .into_iter()
            .flatten()
            .map(move |&i| &graph.edges[i])
    }

    pub fn wires_to(&self, pin: &PinRef) -> impl Iterator<Item = &'g Edge> + '_ {
        let graph = self.graph;
        self.wires_to
            .get(pin)
            .into_iter()
            .flatten()
            .map(move |&i| &graph.edges[i])
    }

    fn child_of_kind(
        &self,
        container: NodeId,
        kind: NodeKind,
    ) -> impl Iterator<Item = NodeId> + '_ {
        self.children(Some(container))
            .iter()
            .copied()
            .filter(move |&c| self.node(c).is_some_and(|n| n.kind == kind))
    }

    pub fn group_input(&self, container: NodeId) -> Option<NodeId> {
        self.child_of_kind(container, NodeKind::GroupInput).next()
    }

    pub fn group_outputs(&self, container: NodeId) -> Vec<NodeId> {
        self.child_of_kind(container, NodeKind::GroupOutput)
            .collect()
    }

    /// Ancestors from the parent outward. Stops on a containment cycle instead of looping.
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = self.node(id).and_then(|n| n.parent);
        while let Some(p) = cur {
            if p == id || out.contains(&p) {
                break;
            }
            out.push(p);
            cur = self.node(p).and_then(|n| n.parent);
        }
        out
    }

    /// The nearest ancestor (or the node itself) of a kind, e.g. a call site's function.
    pub fn enclosing(&self, id: NodeId, pred: impl Fn(&Node) -> bool) -> Option<NodeId> {
        std::iter::once(id)
            .chain(self.ancestors(id))
            .find(|&a| self.node(a).is_some_and(&pred))
    }

    /// Every node below `id`, excluding synthesized interface/dispatch nodes.
    pub fn member_count(&self, id: NodeId) -> usize {
        self.children(Some(id))
            .iter()
            .filter(|&&c| self.node(c).is_some_and(|n| !n.kind.is_synthesized()))
            .map(|&c| 1 + self.member_count(c))
            .sum()
    }
}
