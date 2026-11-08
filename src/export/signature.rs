//! Function parameter and return pins, spelled as written. No type resolution happens here.

use argo_protocol::{NodeId, PinSpec, ValueType};

use super::Exporter;
use crate::graph::GraphNode;
use crate::{CNodeRole, CNodeType};

const SPECIFIERS: &[&str] = &["static", "inline", "extern", "__inline", "__inline__"];

impl Exporter<'_> {
    pub(super) fn signature(&mut self, n: &GraphNode, node: NodeId) {
        let Some(&f) = self.functions.get(&(n.span.lo, n.span.hi)) else {
            return;
        };
        let Some(sig) = f
            .children
            .iter()
            .find(|c| c.role == Some(CNodeRole::Signature))
        else {
            return;
        };
        let name = f.name.as_deref().unwrap_or("");
        // Return type: everything before `name(`, minus storage/inline specifiers.
        let head = self.text(f.span.lo, sig.span.hi);
        let ret = head
            .split(&format!("{name}("))
            .next()
            .unwrap_or("")
            .split_whitespace()
            .filter(|w| !SPECIFIERS.contains(w))
            .collect::<Vec<_>>()
            .join(" ");
        for p in sig
            .children
            .iter()
            .filter(|c| c.node_type == CNodeType::Parameter)
        {
            let text = self.text(p.span.lo, p.span.hi);
            let (pname, spelled) = match &p.name {
                Some(pn) => {
                    let ty = text.trim_end().strip_suffix(pn.as_str()).unwrap_or(&text);
                    (pn.clone(), ty.trim().to_string())
                }
                None => (format!("arg{}", p.span.lo), text.trim().to_string()),
            };
            self.b.pin(
                node,
                PinSpec::param(&pname, ValueType::Spelled { c: spelled }),
            );
        }
        if !ret.is_empty() && ret != "void" {
            self.b
                .pin(node, PinSpec::ret(ValueType::Spelled { c: ret }));
        }
    }
}
