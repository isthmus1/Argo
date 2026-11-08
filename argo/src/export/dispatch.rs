//! Exec wiring through a branch, loop or switch dispatch node: one exec output per arm.

use argo_protocol::{NodeId, PinSpec, pin::key};

use super::Exporter;
use super::flow::Exit;
use super::interior::Outputs;
use crate::CNodeRole;
use crate::graph::GraphNodeKind;

impl Exporter<'_> {
    /// `true` runs the first arm; `false` runs the `else` arm, or completes when there is none.
    pub(super) fn branch(
        &mut self,
        gi: NodeId,
        d: NodeId,
        kids: &[(usize, NodeId)],
        out: &mut Outputs,
    ) {
        let arms = [key::arm(0), key::ELSE.to_string()];
        self.b.pins(
            d,
            [
                PinSpec::enter(),
                PinSpec::arm(arms[0].as_str(), "true"),
                PinSpec::arm(key::ELSE, "false"),
            ],
        );
        self.b.exec((gi, key::ENTER), (d, key::ENTER));
        for (i, arm) in arms.iter().enumerate() {
            match kids.get(i) {
                Some(&(c, cn)) => {
                    self.b.exec((d, arm), (cn, key::ENTER));
                    if self.falls_through(c) {
                        out.to(self.b, (cn, key::THEN), key::THEN);
                    }
                    self.route_exits(c, cn, out, None);
                }
                None => out.to(self.b, (d, arm), key::THEN),
            }
        }
    }

    /// The body runs from `body` and returns to `exec.next`; `completed` leaves the loop.
    pub(super) fn loop_body(
        &mut self,
        gi: NodeId,
        d: NodeId,
        kids: &[(usize, NodeId)],
        out: &mut Outputs,
    ) {
        self.b.pins(
            d,
            [
                PinSpec::enter(),
                PinSpec::backedge(),
                PinSpec::body(),
                PinSpec::completed(),
            ],
        );
        self.b.exec((gi, key::ENTER), (d, key::ENTER));
        out.to(self.b, (d, key::COMPLETED), key::THEN);
        let mut cur = Some((d, key::BODY));
        for &(c, cn) in kids {
            if let Some(from) = cur.take() {
                self.b.exec(from, (cn, key::ENTER));
            }
            cur = self.falls_through(c).then_some((cn, key::THEN));
            self.route_exits(c, cn, out, Some(d));
        }
        if let Some(from) = cur {
            self.b.exec(from, (d, key::NEXT));
        }
    }

    /// One arm per case group, plus `default` straight to completion when no group has it.
    pub(super) fn switch(
        &mut self,
        gi: NodeId,
        d: NodeId,
        kids: &[(usize, NodeId)],
        out: &mut Outputs,
    ) {
        self.b.pin(d, PinSpec::enter());
        let (mut has_default, mut case) = (false, 0);
        for &(c, cn) in kids {
            let group = &self.g.nodes[c];
            if group.kind != GraphNodeKind::CaseGroup {
                continue;
            }
            let arm = if group
                .parts
                .iter()
                .any(|p| p.role == CNodeRole::DefaultLabel)
            {
                has_default = true;
                key::DEFAULT.to_string()
            } else {
                case += 1;
                key::case(case - 1)
            };
            let label = self.case_label(group);
            self.b.pin(d, PinSpec::arm(arm.as_str(), &label));
            self.b.exec((d, &arm), (cn, key::ENTER));
            if self.falls_through(c) {
                out.to(self.b, (cn, key::THEN), key::THEN);
            }
            for exit in self.escaping(c) {
                let target = if exit == Exit::Break {
                    key::THEN
                } else {
                    out.exit_key(exit)
                };
                out.to(self.b, (cn, exit.key()), target);
            }
        }
        if !has_default {
            self.b.pin(d, PinSpec::arm(key::DEFAULT, "default"));
            out.to(self.b, (d, key::DEFAULT), key::THEN);
        }
        self.b.exec((gi, key::ENTER), (d, key::ENTER));
    }
}
