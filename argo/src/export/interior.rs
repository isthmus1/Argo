//! Inside an opened container: group input, group outputs (one per exit), and exec order.

use std::collections::HashMap;

use argo_protocol::{DispatchKind, GraphBuilder, NodeId, NodeKind, Provenance, pin::key};

use super::Exporter;
use super::flow::{Exit, is_exec};
use crate::CNodeRole;

/// The container's group outputs, created on first use: one per exec exit key.
pub(super) struct Outputs {
    container: NodeId,
    /// A function's `return` is its completion, carrying the return value.
    function: bool,
    by_exit: HashMap<&'static str, NodeId>,
}

impl Outputs {
    /// Wire `from` to the group output for the container's exec output `exit`.
    pub(super) fn to(&mut self, b: &mut GraphBuilder, from: (NodeId, &str), exit: &'static str) {
        let values: &[&str] = if self.function { &[key::RETURN] } else { &[] };
        let (container, function) = (self.container, self.function);
        let go = *self.by_exit.entry(exit).or_insert_with(|| {
            let go = b.group_output(container, exit, values);
            if function {
                typed_result(b, go);
            }
            go
        });
        b.exec(from, (go, exit));
    }

    /// Where a child's exit leaves this container.
    pub(super) fn exit_key(&self, exit: Exit) -> &'static str {
        if self.function { key::THEN } else { exit.key() }
    }
}

impl Exporter<'_> {
    pub(super) fn interior(&mut self, id: usize, node: NodeId, kind: NodeKind) {
        let g = self.g;
        let n = &g.nodes[id];
        let gi = self.b.group_input(node);
        if kind == NodeKind::Function {
            self.declare_params(gi);
        }
        let mut out = Outputs {
            container: node,
            function: kind == NodeKind::Function,
            by_exit: HashMap::new(),
        };
        // The dispatch comes before the arms in sibling order.
        let dispatch = match kind {
            NodeKind::Branch => Some(DispatchKind::Branch),
            NodeKind::Loop(_) => Some(DispatchKind::Loop),
            NodeKind::Switch => Some(DispatchKind::Switch),
            _ => None,
        }
        .map(|d| {
            let header = n
                .parts
                .iter()
                .find(|p| matches!(p.role, CNodeRole::Condition | CNodeRole::Selector))
                .map_or(self.range(n.span.lo, n.span.hi), |p| {
                    self.range(p.span.lo, p.span.hi)
                });
            let dn = self
                .b
                .node(Some(node), NodeKind::Dispatch(d), self.header(n));
            let mut prov = Provenance::at(header);
            prov.parts = self.parts(n);
            self.b.provenance(dn, prov);
            dn
        });
        let kids: Vec<(usize, NodeId)> = n
            .children
            .iter()
            .map(|&c| (c, self.node(c, node)))
            .collect();
        match (kind, dispatch) {
            (NodeKind::Branch, Some(d)) => self.branch(gi, d, &kids, &mut out),
            (NodeKind::Loop(_), Some(d)) => self.loop_body(gi, d, &kids, &mut out),
            (NodeKind::Switch, Some(d)) => self.switch(gi, d, &kids, &mut out),
            _ => self.sequential(gi, &kids, &mut out),
        }
    }

    /// Inside a function, its input reads like the parameter list: `struct ram *r`.
    fn declare_params(&mut self, gi: NodeId) {
        for spec in self.b.specs(gi) {
            let (Some(name), Some(ty)) = (
                spec.key.as_str().strip_prefix("param."),
                spec.kind.value_type(),
            ) else {
                continue;
            };
            let ty = ty.c();
            let label = if ty.ends_with('*') {
                format!("{ty}{name}")
            } else {
                format!("{ty} {name}")
            };
            self.b
                .relabel(&argo_protocol::PinRef::new(gi, spec.key.clone()), label);
        }
    }

    /// Statement order is exec order.
    fn sequential(&mut self, gi: NodeId, kids: &[(usize, NodeId)], out: &mut Outputs) {
        let mut cur = Some((gi, key::ENTER));
        for &(c, cn) in kids {
            if !is_exec(self.g.nodes[c].kind) {
                continue;
            }
            if let Some(from) = cur.take() {
                self.b.exec(from, (cn, key::ENTER));
            }
            cur = self.falls_through(c).then_some((cn, key::THEN));
            self.route_exits(c, cn, out, None);
        }
        if let Some(from) = cur {
            out.to(self.b, from, key::THEN);
        }
    }

    /// Send a child's escaping exits on: inside a loop, `continue` re-enters the dispatch and
    /// `break` completes the loop; anything else leaves through the container's matching exit.
    pub(super) fn route_exits(
        &mut self,
        c: usize,
        cn: NodeId,
        out: &mut Outputs,
        loop_dispatch: Option<NodeId>,
    ) {
        for exit in self.escaping(c) {
            match (exit, loop_dispatch) {
                (Exit::Continue, Some(d)) => {
                    self.b.exec((cn, exit.key()), (d, key::NEXT));
                }
                (Exit::Break, Some(_)) => out.to(self.b, (cn, exit.key()), key::THEN),
                _ => {
                    let target = out.exit_key(exit);
                    out.to(self.b, (cn, exit.key()), target);
                }
            }
        }
    }
}

/// Inside a function, its result reads like a declaration: `int result`.
fn typed_result(b: &mut GraphBuilder, go: NodeId) {
    for spec in b.specs(go) {
        if spec.key.as_str() == key::RETURN
            && let Some(ty) = spec.kind.value_type()
        {
            let label = format!("{} result", ty.c());
            b.relabel(&argo_protocol::PinRef::new(go, spec.key.clone()), label);
        }
    }
}
