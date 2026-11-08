use serde::{Deserialize, Serialize};

use crate::{Access, EdgeId, NodeId, PinRef};

/// A non-containment relation. Exec and data wires join pins of sibling nodes; call, type-use,
/// state and callback relations join nodes anywhere in the hierarchy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub relation: Relation,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Relation {
    /// Execution order: control pin to control pin. This is also how control exits leave a region.
    Exec { from: PinRef, to: PinRef },
    /// Where a value, state handle or memory handle comes from. Never decides when anything runs.
    Data { from: PinRef, to: PinRef },
    /// A call site (or a function containing an unexpanded call) and its callee function.
    Call { site: NodeId, callee: NodeId },
    /// A node using a type node.
    TypeUse { user: NodeId, ty: NodeId },
    /// A node reading or writing a global/static state node.
    StateUse {
        user: NodeId,
        state: NodeId,
        access: Access,
    },
    /// An indirect call; `target: None` is the explicit unresolved marker (§5.6).
    Callback {
        site: NodeId,
        target: Option<NodeId>,
    },
}

impl Relation {
    /// The pin endpoints of a wire.
    pub fn wire(&self) -> Option<(&PinRef, &PinRef)> {
        match self {
            Self::Exec { from, to } | Self::Data { from, to } => Some((from, to)),
            _ => None,
        }
    }

    /// The node endpoints of any relation.
    pub fn nodes(&self) -> (NodeId, Option<NodeId>) {
        match self {
            Self::Exec { from, to } | Self::Data { from, to } => (from.node, Some(to.node)),
            Self::Call { site, callee } => (*site, Some(*callee)),
            Self::TypeUse { user, ty } => (*user, Some(*ty)),
            Self::StateUse { user, state, .. } => (*user, Some(*state)),
            Self::Callback { site, target } => (*site, *target),
        }
    }
}
