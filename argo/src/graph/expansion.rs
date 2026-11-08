use super::{ControlExit, GraphNodeKind, SourceSpan, SyntaxPart};
use crate::{CNode, CNodeRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsupported(pub &'static str);

/// An unnumbered graph node. The driver supplies identity and containment edges.
pub struct NodeDraft {
    pub kind: GraphNodeKind,
    pub name: Option<String>,
    pub role: Option<CNodeRole>,
    pub span: SourceSpan,
    pub parts: Vec<SyntaxPart>,
    pub exits: Vec<ControlExit>,
    pub diagnostic: Option<String>,
}

impl NodeDraft {
    pub fn new(kind: GraphNodeKind, syntax: &CNode) -> Self {
        Self {
            kind,
            name: syntax.name.clone(),
            role: syntax.role,
            span: syntax.span.into(),
            parts: Vec::new(),
            exits: Vec::new(),
            diagnostic: None,
        }
    }
}

pub struct Expansion<'a> {
    /// None represents a declaration-only target, not a visual/region node.
    pub node: Option<NodeDraft>,
    pub children: Vec<ChildRegion<'a>>,
}

impl<'a> Expansion<'a> {
    pub fn leaf(kind: GraphNodeKind, syntax: &CNode) -> Self {
        Self {
            node: Some(NodeDraft::new(kind, syntax)),
            children: Vec::new(),
        }
    }

    pub fn opaque(syntax: &CNode, reason: impl Into<String>) -> Self {
        let mut expansion = Self::leaf(GraphNodeKind::Opaque, syntax);
        expansion.node.as_mut().unwrap().diagnostic = Some(reason.into());
        expansion
    }
}

pub enum ChildRegion<'a> {
    Syntax(&'a CNode),
    Sequence(Vec<&'a CNode>),
    CaseGroup {
        labels: Vec<&'a CNode>,
        statements: Vec<&'a CNode>,
    },
}
