//! Node kinds and display names for exported syntax nodes.

use argo_protocol::{LoopKind, NodeKind, SyntaxRole, TypeKind};

use super::Exporter;
use crate::CNodeRole;
use crate::graph::{GraphNode, GraphNodeKind};

pub(super) fn kind(n: &GraphNode) -> NodeKind {
    let t = n.template.as_str();
    match n.kind {
        GraphNodeKind::Function => NodeKind::Function,
        GraphNodeKind::Type if t.contains("struct") => NodeKind::Type(TypeKind::Struct),
        GraphNodeKind::Type if t.contains("union") => NodeKind::Type(TypeKind::Union),
        GraphNodeKind::Type if t.contains("enum") => NodeKind::Type(TypeKind::Enum),
        GraphNodeKind::Type => NodeKind::Type(TypeKind::Alias),
        GraphNodeKind::State => NodeKind::State,
        GraphNodeKind::Branch => NodeKind::Branch,
        GraphNodeKind::Loop if t.contains("do") => NodeKind::Loop(LoopKind::DoWhile),
        GraphNodeKind::Loop if t.contains("while") => NodeKind::Loop(LoopKind::While),
        GraphNodeKind::Loop => NodeKind::Loop(LoopKind::For),
        GraphNodeKind::Switch => NodeKind::Switch,
        GraphNodeKind::CaseGroup => NodeKind::CaseGroup,
        GraphNodeKind::Scope => NodeKind::Scope,
        GraphNodeKind::Sequence => NodeKind::Sequence,
        GraphNodeKind::Opaque => NodeKind::Opaque,
    }
}

pub(super) fn role(r: CNodeRole) -> SyntaxRole {
    match r {
        CNodeRole::Signature => SyntaxRole::Signature,
        CNodeRole::Initializer => SyntaxRole::Initializer,
        CNodeRole::Condition => SyntaxRole::Condition,
        CNodeRole::Step => SyntaxRole::Step,
        CNodeRole::Selector => SyntaxRole::Selector,
        CNodeRole::Body => SyntaxRole::Body,
        CNodeRole::Then => SyntaxRole::Then,
        CNodeRole::Else => SyntaxRole::Else,
        CNodeRole::CaseValue => SyntaxRole::CaseValue,
        CNodeRole::CaseEnd => SyntaxRole::CaseEnd,
        CNodeRole::DefaultLabel => SyntaxRole::DefaultLabel,
    }
}

impl Exporter<'_> {
    /// `case 0, 1, 2`, `default`, or both for a group that holds `default` among cases.
    pub(super) fn case_label(&self, n: &GraphNode) -> String {
        let values: Vec<_> = n
            .parts
            .iter()
            .filter(|p| p.role == CNodeRole::CaseValue)
            .map(|p| self.text(p.span.lo, p.span.hi))
            .collect();
        let default = n.parts.iter().any(|p| p.role == CNodeRole::DefaultLabel);
        match (values.is_empty(), default) {
            (true, _) => "default".into(),
            (false, false) => format!("case {}", values.join(", ")),
            (false, true) => format!("case {}, default", values.join(", ")),
        }
    }

    pub(super) fn name(&self, n: &GraphNode) -> String {
        match (n.kind, &n.name) {
            (GraphNodeKind::Scope, _) | (_, None) => {}
            (_, Some(name)) => return name.clone(),
        }
        match n.kind {
            GraphNodeKind::CaseGroup => self.case_label(n),
            GraphNodeKind::Sequence | GraphNodeKind::Opaque => {
                // The first statement, trimmed to a header-sized label.
                let text = self.text(n.span.lo, n.span.hi);
                let line = text
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_end_matches(';');
                if line.chars().count() > 32 {
                    format!("{}…", line.chars().take(31).collect::<String>())
                } else {
                    line.to_string()
                }
            }
            _ => n.name.clone().unwrap_or_else(|| n.template.clone()),
        }
    }
}
