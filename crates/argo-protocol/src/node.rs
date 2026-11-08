use serde::{Deserialize, Serialize};

use crate::{Effect, InstanceId, LogicalKey, NodeId, NodeKind, Provenance, TemplateId};

/// One node of a graph revision. Its interface is its [`Pin`](crate::Pin)s; its migration state is
/// a separate [`NodeStatus`](crate::NodeStatus), because that changes while the revision is frozen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub logical: LogicalKey,
    /// Separate identity for each translation-unit/template configuration (§4).
    pub instance: Option<InstanceId>,
    /// Containment is one tree; every other relation is an [`Edge`](crate::Edge).
    pub parent: Option<NodeId>,
    /// Position among siblings: statement order inside a body, declaration order at top level.
    pub order: u32,
    pub kind: NodeKind,
    pub template: TemplateId,
    /// Display name, e.g. `compute`, `for`, `acc + i`.
    pub name: String,
    /// `None` only for synthesized interface/dispatch nodes and the program root.
    pub provenance: Option<Provenance>,
    /// Translated as its own job (§5.3). Functions always are; synthesized nodes never are.
    pub work_unit: bool,
    pub effects: Vec<Effect>,
}

impl Node {
    pub fn is_pure(&self) -> bool {
        crate::effect::is_pure(&self.effects)
    }
}
