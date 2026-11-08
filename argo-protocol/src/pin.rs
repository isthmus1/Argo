//! Pins: a node's whole interface. Nothing reaches a node except through one of its pins.
//!
//! # Families
//!
//! [`Family::Control`] pins carry execution order; [`Family::Data`] pins carry values, state
//! handles and memory handles. Wires never join the two (§5.4).
//!
//! # Key conventions
//!
//! A [`PinKey`] is unique within its node and stable across revisions, so `(logical node, key)` is
//! a pin's persistent identity. Keys are structural, not display text:
//!
//! | Key | Family | Meaning |
//! | --- | --- | --- |
//! | `exec.in` | control | entry |
//! | `exec.next` | control | loop backedge into a loop dispatch (`continue`, end of body) |
//! | `exec.then` | control | fallthrough / normal completion |
//! | `exit.return`, `exit.break`, `exit.continue`, `exit.goto.<label>` | control | other ways out |
//! | `arm.<n>`, `arm.else`, `case.<n>`, `default`, `body`, `completed` | control | dispatch arms |
//! | `param.<name>`, `return` | data | function parameters and return value |
//! | `live.<var>` | data | region live-in (`in`), live-out (`out`) or both (`in_out`) |
//! | `init.<var>`, `next.<var>` | data | a loop-carried variable's first and next value |
//! | `cond`, `cond.<n>`, `selector` | data | dispatch header values |
//! | `state.<name>`, `mem.<name>` | data | global/static state and pointed-to memory handles |
//! | `field.<name>`, `variant.<name>` | data | struct/union fields and enum variants |
//! | `operand.<n>`, `value` | data | an expression's operands and result |
//!
//! A container's interior mirrors its pins by key: each `in`/`in_out` pin reappears as an output
//! of its [`GroupInput`](crate::NodeKind::GroupInput), and each `out`/`in_out` pin as an input of
//! a [`GroupOutput`](crate::NodeKind::GroupOutput). A call site mirrors its callee's pins.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Access, NodeId, ValueType};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PinKey(pub String);

impl PinKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PinKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for PinKey {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<String> for PinKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

pub mod key {
    pub const ENTER: &str = "exec.in";
    pub const NEXT: &str = "exec.next";
    pub const THEN: &str = "exec.then";
    pub const RETURN_EXIT: &str = "exit.return";
    pub const BREAK: &str = "exit.break";
    pub const CONTINUE: &str = "exit.continue";
    pub const BODY: &str = "body";
    pub const COMPLETED: &str = "completed";
    pub const ELSE: &str = "arm.else";
    pub const DEFAULT: &str = "default";
    pub const RETURN: &str = "return";
    pub const COND: &str = "cond";
    pub const SELECTOR: &str = "selector";
    pub const VALUE: &str = "value";

    pub fn operand(n: usize) -> String {
        format!("operand.{n}")
    }
    pub fn goto(label: &str) -> String {
        format!("exit.goto.{label}")
    }
    pub fn arm(n: usize) -> String {
        format!("arm.{n}")
    }
    pub fn case(n: usize) -> String {
        format!("case.{n}")
    }
    pub fn cond(n: usize) -> String {
        format!("cond.{n}")
    }
    pub fn param(name: &str) -> String {
        format!("param.{name}")
    }
    pub fn live(var: &str) -> String {
        format!("live.{var}")
    }
    pub fn init(var: &str) -> String {
        format!("init.{var}")
    }
    pub fn next(var: &str) -> String {
        format!("next.{var}")
    }
    pub fn state(name: &str) -> String {
        format!("state.{name}")
    }
    pub fn mem(name: &str) -> String {
        format!("mem.{name}")
    }
    pub fn field(name: &str) -> String {
        format!("field.{name}")
    }
    pub fn variant(name: &str) -> String {
        format!("variant.{name}")
    }
}

/// A readable label for a pin key, for records published without one. Producers should label
/// pins themselves; this keeps old snapshots legible.
pub fn label_for_key(key: &str) -> String {
    match key {
        key::ENTER => "run".into(),
        key::THEN => "then".into(),
        key::NEXT => "next iteration".into(),
        key::RETURN => "result".into(),
        key::VALUE => "value".into(),
        key::ELSE => "false".into(),
        k => match k.split_once('.') {
            Some(("exit", rest)) => rest.replace('.', " "),
            Some(("init", var)) => format!("initial {var}"),
            Some(("next", var)) => format!("next {var}"),
            Some(("arm", n)) => format!("arm {n}"),
            Some(("case", n)) => format!("case {n}"),
            Some(("operand", n)) => format!("operand {n}"),
            Some((_, name)) => name.into(),
            None => k.into(),
        },
    }
}

/// Pin direction. An `in_out` pin is drawn as a paired input and output handle on one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    In,
    Out,
    InOut,
}

impl Direction {
    /// Can be the target of a wire.
    pub fn is_input(self) -> bool {
        matches!(self, Self::In | Self::InOut)
    }

    /// Can be the source of a wire.
    pub fn is_output(self) -> bool {
        matches!(self, Self::Out | Self::InOut)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Control,
    Data,
}

/// What an exec pin means for control flow.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "flow")]
pub enum Flow {
    Entry,
    /// Re-entry of a loop dispatch for the next iteration.
    Backedge,
    Then,
    Return,
    Break,
    Continue,
    Goto {
        label: String,
    },
    /// A branch arm, case group or `default`.
    Arm,
    Body,
    Completed,
}

impl Flow {
    /// A control exit of a region (§5.4), other than plain fallthrough.
    pub fn is_exit(&self) -> bool {
        matches!(
            self,
            Self::Return | Self::Break | Self::Continue | Self::Goto { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "pin")]
pub enum PinKind {
    Exec {
        flow: Flow,
    },
    Value {
        ty: ValueType,
    },
    /// A handle to global or static state, not a copied value. `resource` is the state node.
    State {
        resource: Option<NodeId>,
        access: Access,
    },
    /// Memory reached through a pointer (or an escaped local's address), not a copied value.
    Memory {
        pointee: ValueType,
        access: Access,
    },
    /// A function pointer: known target when resolved, otherwise explicitly unresolved.
    Callback {
        target: Option<NodeId>,
    },
}

impl PinKind {
    pub fn family(&self) -> Family {
        match self {
            Self::Exec { .. } => Family::Control,
            _ => Family::Data,
        }
    }

    pub fn flow(&self) -> Option<&Flow> {
        match self {
            Self::Exec { flow } => Some(flow),
            _ => None,
        }
    }

    pub fn value_type(&self) -> Option<&ValueType> {
        match self {
            Self::Value { ty } => Some(ty),
            Self::Memory { pointee, .. } => Some(pointee),
            _ => None,
        }
    }

    /// Wire compatibility between a source and a target pin.
    pub fn connects_to(&self, target: &PinKind) -> bool {
        match (self, target) {
            (Self::Exec { .. }, Self::Exec { .. }) => true,
            (Self::Value { ty: a }, Self::Value { ty: b }) => a.compatible(b),
            (Self::State { resource: a, .. }, Self::State { resource: b, .. }) => {
                a.is_none() || b.is_none() || a == b
            }
            (Self::Memory { pointee: a, .. }, Self::Memory { pointee: b, .. }) => a.compatible(b),
            (Self::Callback { .. }, Self::Callback { .. }) => true,
            _ => false,
        }
    }
}

/// Which analysis produced a pin (§4: signature- or analysis-derived origin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinOrigin {
    /// From a declaration's signature.
    Signature,
    /// From live-in/live-out analysis of a region.
    Liveness,
    /// From a lexically present control exit.
    ControlExit,
    /// Fields, variants and macro parameters.
    Declaration,
    /// A dispatch header value (condition, selector).
    Header,
    /// Copied from another node: group input/output and call sites.
    Mirror,
    /// Derived from syntax alone (entry/fallthrough on a syntax-only graph).
    Syntax,
}

/// A pin before it is attached to a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinSpec {
    pub key: PinKey,
    /// Display text; may be empty for a node's single unnamed output.
    pub label: String,
    pub direction: Direction,
    pub kind: PinKind,
    pub origin: PinOrigin,
}

impl PinSpec {
    pub fn new(
        key: impl Into<PinKey>,
        label: impl Into<String>,
        direction: Direction,
        kind: PinKind,
        origin: PinOrigin,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            direction,
            kind,
            origin,
        }
    }

    fn exec(
        key: impl Into<PinKey>,
        label: &str,
        direction: Direction,
        flow: Flow,
        origin: PinOrigin,
    ) -> Self {
        Self::new(key, label, direction, PinKind::Exec { flow }, origin)
    }

    fn value(
        key: String,
        label: &str,
        direction: Direction,
        ty: ValueType,
        origin: PinOrigin,
    ) -> Self {
        Self::new(key, label, direction, PinKind::Value { ty }, origin)
    }

    pub fn enter() -> Self {
        Self::exec(
            key::ENTER,
            "run",
            Direction::In,
            Flow::Entry,
            PinOrigin::Syntax,
        )
    }
    pub fn then() -> Self {
        Self::exec(
            key::THEN,
            "then",
            Direction::Out,
            Flow::Then,
            PinOrigin::Syntax,
        )
    }
    pub fn backedge() -> Self {
        Self::exec(
            key::NEXT,
            "next iteration",
            Direction::In,
            Flow::Backedge,
            PinOrigin::Syntax,
        )
    }
    pub fn exit(flow: Flow) -> Self {
        let (key, label) = match &flow {
            Flow::Return => (key::RETURN_EXIT.to_string(), "return".to_string()),
            Flow::Break => (key::BREAK.to_string(), "break".to_string()),
            Flow::Continue => (key::CONTINUE.to_string(), "continue".to_string()),
            Flow::Goto { label } => (key::goto(label), format!("goto {label}")),
            other => panic!("{other:?} is not a control exit"),
        };
        Self::exec(key, &label, Direction::Out, flow, PinOrigin::ControlExit)
    }
    pub fn arm(key: impl Into<PinKey>, label: &str) -> Self {
        Self::exec(key, label, Direction::Out, Flow::Arm, PinOrigin::Header)
    }
    pub fn body() -> Self {
        Self::exec(
            key::BODY,
            "loop body",
            Direction::Out,
            Flow::Body,
            PinOrigin::Header,
        )
    }
    pub fn completed() -> Self {
        Self::exec(
            key::COMPLETED,
            "completed",
            Direction::Out,
            Flow::Completed,
            PinOrigin::Header,
        )
    }

    pub fn param(name: &str, ty: ValueType) -> Self {
        Self::value(
            key::param(name),
            name,
            Direction::In,
            ty,
            PinOrigin::Signature,
        )
    }
    /// The value leaving with `return`: a function's result, or a region's when it returns.
    pub fn ret(ty: ValueType) -> Self {
        Self::value(
            key::RETURN.into(),
            "result",
            Direction::Out,
            ty,
            PinOrigin::Signature,
        )
    }
    /// A region's value: `In` for live-in, `Out` for live-out, `InOut` for both.
    pub fn live(var: &str, direction: Direction, ty: ValueType) -> Self {
        Self::value(key::live(var), var, direction, ty, PinOrigin::Liveness)
    }
    pub fn init(var: &str, ty: ValueType) -> Self {
        Self::value(
            key::init(var),
            &format!("initial {var}"),
            Direction::In,
            ty,
            PinOrigin::Liveness,
        )
    }
    pub fn next(var: &str, ty: ValueType) -> Self {
        Self::value(
            key::next(var),
            &format!("next {var}"),
            Direction::In,
            ty,
            PinOrigin::Liveness,
        )
    }
    pub fn cond() -> Self {
        Self::value(
            key::COND.into(),
            "condition",
            Direction::In,
            ValueType::Bool,
            PinOrigin::Header,
        )
    }
    pub fn selector(ty: ValueType) -> Self {
        Self::value(
            key::SELECTOR.into(),
            "selector",
            Direction::In,
            ty,
            PinOrigin::Header,
        )
    }
    pub fn state(name: &str, resource: Option<NodeId>, access: Access) -> Self {
        let direction = if access == Access::Read {
            Direction::In
        } else {
            Direction::InOut
        };
        Self::new(
            key::state(name),
            name,
            direction,
            PinKind::State { resource, access },
            PinOrigin::Liveness,
        )
    }
    pub fn memory(name: &str, pointee: ValueType, access: Access) -> Self {
        let direction = if access == Access::Read {
            Direction::In
        } else {
            Direction::InOut
        };
        Self::new(
            key::mem(name),
            name,
            direction,
            PinKind::Memory { pointee, access },
            PinOrigin::Signature,
        )
    }
    pub fn operand(n: usize, label: &str, ty: ValueType) -> Self {
        Self::value(key::operand(n), label, Direction::In, ty, PinOrigin::Syntax)
    }
    pub fn value_out(ty: ValueType) -> Self {
        Self::value(
            key::VALUE.into(),
            "value",
            Direction::Out,
            ty,
            PinOrigin::Syntax,
        )
    }
    pub fn field(name: &str, ty: ValueType) -> Self {
        Self::value(
            key::field(name),
            name,
            Direction::Out,
            ty,
            PinOrigin::Declaration,
        )
    }
    pub fn variant(name: &str, ty: ValueType) -> Self {
        Self::value(
            key::variant(name),
            name,
            Direction::Out,
            ty,
            PinOrigin::Declaration,
        )
    }

    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn origin(mut self, origin: PinOrigin) -> Self {
        self.origin = origin;
        self
    }

    pub fn at(self, node: NodeId) -> Pin {
        Pin {
            node,
            key: self.key,
            label: self.label,
            direction: self.direction,
            kind: self.kind,
            origin: self.origin,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub node: NodeId,
    pub key: PinKey,
    pub label: String,
    pub direction: Direction,
    pub kind: PinKind,
    pub origin: PinOrigin,
}

impl Pin {
    pub fn family(&self) -> Family {
        self.kind.family()
    }

    pub fn pin_ref(&self) -> PinRef {
        PinRef {
            node: self.node,
            key: self.key.clone(),
        }
    }

    pub fn spec(&self) -> PinSpec {
        PinSpec {
            key: self.key.clone(),
            label: self.label.clone(),
            direction: self.direction,
            kind: self.kind.clone(),
            origin: self.origin,
        }
    }
}

/// A pin's address within a revision.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PinRef {
    pub node: NodeId,
    pub key: PinKey,
}

impl PinRef {
    pub fn new(node: NodeId, key: impl Into<PinKey>) -> Self {
        Self {
            node,
            key: key.into(),
        }
    }
}

impl fmt::Display for PinRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.node.0, self.key)
    }
}
