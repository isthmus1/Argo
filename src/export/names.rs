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

    /// A node's header: what kind of construct it is, in the source's own words.
    pub(super) fn name(&self, n: &GraphNode) -> String {
        let header = self.header(n);
        match n.kind {
            GraphNodeKind::Branch if n.children.len() > 1 => format!("{header} … else"),
            _ => header,
        }
    }

    /// The construct's C header, e.g. `if (i & 1)` or `for (int i = 0; i < n; i++)`. A dispatch
    /// node shows this; its region adds `… else` when it has one.
    pub(super) fn header(&self, n: &GraphNode) -> String {
        let part = |role| {
            n.parts
                .iter()
                .find(|p| p.role == role)
                .map(|p| collapse(&self.text(p.span.lo, p.span.hi)))
                .unwrap_or_default()
        };
        let text = match (n.kind, kind(n)) {
            (GraphNodeKind::Branch, _) => format!("if ({})", part(CNodeRole::Condition)),
            (_, NodeKind::Loop(LoopKind::For)) => format!(
                "for ({}; {}; {})",
                part(CNodeRole::Initializer).trim_end_matches(';'),
                part(CNodeRole::Condition),
                part(CNodeRole::Step)
            ),
            (_, NodeKind::Loop(LoopKind::While)) => format!("while ({})", part(CNodeRole::Condition)),
            (_, NodeKind::Loop(LoopKind::DoWhile)) => {
                format!("do … while ({})", part(CNodeRole::Condition))
            }
            (GraphNodeKind::Switch, _) => format!("switch ({})", part(CNodeRole::Selector)),
            (GraphNodeKind::CaseGroup, _) => format!("{}:", self.case_label(n)),
            (GraphNodeKind::Scope, _) if n.name.as_deref() == Some("else") => "else".into(),
            (GraphNodeKind::Scope, _) => "{ … }".into(),
            (GraphNodeKind::Sequence | GraphNodeKind::Opaque, _) => {
                collapse(&self.text(n.span.lo, n.span.hi)).trim_end_matches(';').to_string()
            }
            _ => n.name.clone().unwrap_or_else(|| n.template.clone()),
        };
        clip(&text, HEADER_MAX)
    }
}

/// Header text longer than this is cut with an ellipsis.
const HEADER_MAX: usize = 44;

/// Source text on one line: newlines and runs of spaces become single spaces.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max - 1).collect::<String>().trim_end())
    } else {
        text.to_string()
    }
}
