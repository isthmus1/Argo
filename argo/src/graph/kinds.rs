use serde::Serialize;

/// Navigation/provenance regions, not executable or independently safe work units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeKind {
    Function,
    Type,
    State,
    Branch,
    Loop,
    Switch,
    CaseGroup,
    Scope,
    Sequence,
    Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeKind {
    Contains,
}

/// Lexically present exits only. Targets and executable control flow are unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlExitKind {
    Return,
    Break,
    Continue,
}
