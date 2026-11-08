//! Assembles a revision in memory, deriving IDs, sibling order, logical keys and the mirrored pins
//! of synthesized nodes so producers can't get those conventions subtly wrong.

use std::collections::{HashMap, HashSet};

use crate::{
    Direction, DispatchKind, Edge, EdgeId, Effect, Family, FileRef, Graph, GraphRevision,
    LogicalKey, LoopKind, Node, NodeId, NodeKind, PinOrigin, PinRef, PinSpec, Provenance, Relation,
    TemplateId, TypeKind, TypeLayout,
};

pub struct GraphBuilder {
    graph: Graph,
    next_node: u64,
    next_edge: u64,
    order: HashMap<Option<NodeId>, u32>,
    logical: HashSet<LogicalKey>,
}

/// The template ID recorded when a producer doesn't supply its own.
pub fn default_template(kind: NodeKind) -> TemplateId {
    use NodeKind::*;
    TemplateId::new(match kind {
        Program => "argo.program",
        Module => "argo.module",
        Function => "c.function",
        Type(TypeKind::Struct) => "c.struct",
        Type(TypeKind::Union) => "c.union",
        Type(TypeKind::Enum) => "c.enum",
        Type(TypeKind::Alias) => "c.typedef",
        State => "c.variable",
        Macro => "c.macro",
        MacroGroup => "c.macro_call",
        Branch => "c.if",
        Loop(LoopKind::For) => "c.for",
        Loop(LoopKind::While) => "c.while",
        Loop(LoopKind::DoWhile) => "c.do",
        Switch => "c.switch",
        CaseGroup => "c.case_group",
        Scope => "c.block",
        Sequence => "c.sequence",
        Fragment => "c.fragment",
        Opaque => "c.opaque",
        Call => "c.call",
        Expression => "c.expression",
        Local => "c.declaration",
        Assign => "c.assignment",
        Constant => "c.constant",
        GroupInput => "argo.group_input",
        GroupOutput => "argo.group_output",
        Dispatch(DispatchKind::Branch) => "argo.dispatch.branch",
        Dispatch(DispatchKind::Loop) => "argo.dispatch.loop",
        Dispatch(DispatchKind::Switch) => "argo.dispatch.switch",
    })
}

fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_matches('_');
    if s.is_empty() {
        "node".into()
    } else {
        s.into()
    }
}

impl GraphBuilder {
    pub fn new(revision: GraphRevision) -> Self {
        Self {
            graph: Graph::empty(revision),
            next_node: 0,
            next_edge: 0,
            order: HashMap::new(),
            logical: HashSet::new(),
        }
    }

    pub fn file(&mut self, file: FileRef) {
        self.graph.files.push(file);
    }

    /// Add a node as the next child of `parent`. Functions start as work units; everything else
    /// opts in with [`work_unit`](Self::work_unit).
    pub fn node(
        &mut self,
        parent: Option<NodeId>,
        kind: NodeKind,
        name: impl Into<String>,
    ) -> NodeId {
        let name = name.into();
        let id = NodeId(self.next_node);
        self.next_node += 1;
        let order = self.order.entry(parent).or_default();
        let this_order = *order;
        *order += 1;
        let base = match parent.and_then(|p| self.get(p)) {
            Some(p) => format!("{}/{}", p.logical, slug(&name)),
            None => slug(&name),
        };
        let mut logical = LogicalKey::new(base.clone());
        let mut n = 1;
        while !self.logical.insert(logical.clone()) {
            logical = LogicalKey::new(format!("{base}#{n}"));
            n += 1;
        }
        self.graph.nodes.push(Node {
            id,
            logical,
            instance: None,
            parent,
            order: this_order,
            kind,
            template: default_template(kind),
            name,
            provenance: None,
            work_unit: kind == NodeKind::Function,
            effects: Vec::new(),
        });
        id
    }

    fn get(&self, id: NodeId) -> Option<&Node> {
        self.graph.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.graph
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .expect("node was added by this builder")
    }

    pub fn template(&mut self, id: NodeId, template: impl Into<String>) -> &mut Self {
        self.node_mut(id).template = TemplateId::new(template);
        self
    }

    pub fn provenance(&mut self, id: NodeId, provenance: Provenance) -> &mut Self {
        self.node_mut(id).provenance = Some(provenance);
        self
    }

    pub fn work_unit(&mut self, id: NodeId, work_unit: bool) -> &mut Self {
        self.node_mut(id).work_unit = work_unit;
        self
    }

    pub fn effects(&mut self, id: NodeId, effects: Vec<Effect>) -> &mut Self {
        self.node_mut(id).effects = effects;
        self
    }

    pub fn pin(&mut self, node: NodeId, spec: PinSpec) -> PinRef {
        let pin = spec.at(node);
        let r = pin.pin_ref();
        self.graph.pins.push(pin);
        r
    }

    pub fn pins(&mut self, node: NodeId, specs: impl IntoIterator<Item = PinSpec>) {
        for spec in specs {
            self.pin(node, spec);
        }
    }

    pub fn relation(&mut self, relation: Relation) -> EdgeId {
        let id = EdgeId(self.next_edge);
        self.next_edge += 1;
        self.graph.edges.push(Edge { id, relation });
        id
    }

    pub fn exec(&mut self, from: (NodeId, &str), to: (NodeId, &str)) -> EdgeId {
        self.relation(Relation::Exec {
            from: PinRef::new(from.0, from.1),
            to: PinRef::new(to.0, to.1),
        })
    }

    pub fn data(&mut self, from: (NodeId, &str), to: (NodeId, &str)) -> EdgeId {
        self.relation(Relation::Data {
            from: PinRef::new(from.0, from.1),
            to: PinRef::new(to.0, to.1),
        })
    }

    pub fn layout(&mut self, layout: TypeLayout) {
        self.graph.layouts.push(layout);
    }

    fn specs(&self, node: NodeId) -> Vec<PinSpec> {
        self.graph
            .pins
            .iter()
            .filter(|p| p.node == node)
            .map(|p| p.spec())
            .collect()
    }

    /// The container's group input: every `in`/`in_out` pin re-exposed as an output.
    pub fn group_input(&mut self, container: NodeId) -> NodeId {
        let gi = self.node(Some(container), NodeKind::GroupInput, "Input");
        for spec in self.specs(container) {
            if spec.direction.is_input() {
                // Inside, the container's entry is where its body starts.
                let label = if spec.key.as_str() == crate::pin::key::ENTER {
                    "start".into()
                } else {
                    spec.label.clone()
                };
                self.pin(
                    gi,
                    PinSpec {
                        direction: Direction::Out,
                        origin: PinOrigin::Mirror,
                        label,
                        ..spec
                    },
                );
            }
        }
        gi
    }

    /// One exit path of the container: the exec output `exit`, plus the data outputs in `values`
    /// that leave with it.
    pub fn group_output(&mut self, container: NodeId, exit: &str, values: &[&str]) -> NodeId {
        let go = self.node(Some(container), NodeKind::GroupOutput, "Output");
        for spec in self.specs(container) {
            let wanted = match spec.kind.family() {
                Family::Control => spec.key.as_str() == exit,
                Family::Data => values.contains(&spec.key.as_str()),
            };
            if wanted && spec.direction.is_output() {
                // Inside, falling off the container's end is where its body ends.
                let label = if spec.key.as_str() == crate::pin::key::THEN {
                    "end".into()
                } else {
                    spec.label.clone()
                };
                self.pin(
                    go,
                    PinSpec {
                        direction: Direction::In,
                        origin: PinOrigin::Mirror,
                        label,
                        ..spec
                    },
                );
            }
        }
        go
    }

    /// A call site mirroring `callee`'s pins, plus its `Call` relation. A `data_only` site drops
    /// the exec pins; validation then requires the callee to be pure.
    pub fn call(&mut self, parent: NodeId, callee: NodeId, data_only: bool) -> NodeId {
        let name = self.get(callee).map(|c| c.name.clone()).unwrap_or_default();
        let effects = self
            .get(callee)
            .map(|c| c.effects.clone())
            .unwrap_or_default();
        let site = self.node(Some(parent), NodeKind::Call, name);
        self.node_mut(site).effects = effects;
        for spec in self.specs(callee) {
            if !(data_only && spec.kind.family() == Family::Control) {
                self.pin(
                    site,
                    PinSpec {
                        origin: PinOrigin::Mirror,
                        ..spec
                    },
                );
            }
        }
        self.relation(Relation::Call { site, callee });
        site
    }

    pub fn finish(self) -> Graph {
        self.graph
    }
}
