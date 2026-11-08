//! Node kinds: what a node is, independent of its migration state or language.

use serde::{Deserialize, Serialize};

/// Template catalog v1 (§5.3), plus the program/module levels above functions (§5.5), the
/// statement-level detail nodes drawn inside a sequence, and the interface nodes synthesized
/// inside every opened container. Sources are preprocessed before parsing, so macros are already
/// expanded and have no nodes of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "of")]
pub enum NodeKind {
    // Above functions.
    Program,
    /// Initially one per original source file; becomes a Rust module when it collapses.
    Module,

    // Top-level boundary nodes: pins come from the declaration.
    Function,
    Type(TypeKind),
    /// A global or file/function-static variable: one state pin.
    State,

    // Statement regions: pins come from live-in/live-out and control-exit analysis.
    Branch,
    Loop(LoopKind),
    Switch,
    /// Cases joined by fallthrough stay in one group.
    CaseGroup,
    /// A bare `{ … }` block.
    Scope,
    /// A straight-line run between control statements.
    Sequence,
    /// A fragment file included in a body, one per instantiation.
    Fragment,
    /// Kept whole as one leaf with a diagnostic; translated inside its parent's context.
    Opaque,

    // Statement-level detail inside a sequence. Never their own ABI unless marked work units.
    /// A call site. Its pins mirror the callee's; a `Call` relation names the callee.
    Call,
    /// A side-effect-free operator expression, e.g. `acc + i`.
    Expression,
    /// A local declaration with its initial value.
    Local,
    /// A write to a local.
    Assign,
    /// A literal or enumerator value.
    Constant,

    // Interface nodes synthesized inside an opened container. Never work units.
    /// The container's inputs, re-exposed as outputs. Exactly one per opened container.
    GroupInput,
    /// One exit path of the container: its exec exit plus the values leaving with it. A container
    /// may have several, like several UE5 Return nodes.
    GroupOutput,
    /// The control node inside a branch, loop or switch: one exec output per arm.
    Dispatch(DispatchKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeKind {
    Struct,
    Union,
    Enum,
    Alias,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopKind {
    For,
    While,
    DoWhile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchKind {
    Branch,
    Loop,
    Switch,
}

/// Header colour families. All control flow shares one family; language is shown separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeFamily {
    /// Group input/output: a container's interface.
    Interface,
    /// Branches, loops, switches, their arms and dispatch nodes.
    Control,
    /// Functions, call sites and modules.
    Function,
    /// Sequences, scopes, fragments: straight-line statement regions.
    Statement,
    Expression,
    /// Locals, assignments, constants and global state.
    Variable,
    Type,
    Opaque,
}

impl NodeKind {
    pub fn family(self) -> NodeFamily {
        use NodeKind::*;
        match self {
            GroupInput | GroupOutput => NodeFamily::Interface,
            Branch | Loop(_) | Switch | CaseGroup | Dispatch(_) => NodeFamily::Control,
            Program | Module | Function | Call => NodeFamily::Function,
            Scope | Sequence | Fragment => NodeFamily::Statement,
            Expression => NodeFamily::Expression,
            State | Local | Assign | Constant => NodeFamily::Variable,
            Type(_) => NodeFamily::Type,
            Opaque => NodeFamily::Opaque,
        }
    }

    /// Data-only kinds (UE5 pure nodes): never have exec pins. A `Call` may also be data-only,
    /// but only when every effect is `pure` (§5.4); that is checked per node, not per kind.
    pub fn is_data_only(self) -> bool {
        use NodeKind::*;
        matches!(self, Type(_) | State | Expression | Constant | Local)
    }

    /// Kinds that sit on the exec chain and need exactly one exec entry.
    pub fn needs_exec(self) -> bool {
        use NodeKind::*;
        matches!(
            self,
            Function
                | Branch
                | Loop(_)
                | Switch
                | CaseGroup
                | Scope
                | Sequence
                | Fragment
                | Opaque
                | Assign
                | Dispatch(_)
        )
    }

    /// Kinds that may be opened to show children.
    pub fn is_container(self) -> bool {
        use NodeKind::*;
        matches!(
            self,
            Program
                | Module
                | Function
                | Branch
                | Loop(_)
                | Switch
                | CaseGroup
                | Scope
                | Sequence
                | Fragment
        )
    }

    /// Opened containers wire their children through group input/output nodes. Program and
    /// module levels connect top-level items by relations instead (§5.5).
    pub fn has_interior_interface(self) -> bool {
        self.is_container() && !matches!(self, NodeKind::Program | NodeKind::Module)
    }

    pub fn is_interface(self) -> bool {
        matches!(self, NodeKind::GroupInput | NodeKind::GroupOutput)
    }

    /// Synthesized nodes have no source of their own and are never work units.
    pub fn is_synthesized(self) -> bool {
        matches!(
            self,
            NodeKind::GroupInput | NodeKind::GroupOutput | NodeKind::Dispatch(_)
        )
    }
}
