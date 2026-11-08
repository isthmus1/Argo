use serde::Serialize;

use super::{ControlExit, GraphEdgeKind, GraphNodeKind, SourceSpan, SyntaxPart};
use crate::CNodeRole;

/// IDs are deterministic within one graph revision, not persistent logical IDs.
pub type NodeId = usize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Graph {
    pub schema_version: u32,
    pub analysis: &'static str,
    /// Original bytes, rather than lossy UTF-8 or reconstructed C.
    pub source: Vec<u8>,
    pub roots: Vec<NodeId>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub diagnostics: Vec<GraphDiagnostic>,
}

impl Graph {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub kind: GraphNodeKind,
    pub template: String,
    pub name: Option<String>,
    pub role: Option<CNodeRole>,
    pub span: SourceSpan,
    pub parts: Vec<SyntaxPart>,
    pub exits: Vec<ControlExit>,
    pub children: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphEdge {
    pub source: NodeId,
    pub target: NodeId,
    pub kind: GraphEdgeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphDiagnostic {
    pub node: NodeId,
    pub message: String,
}
