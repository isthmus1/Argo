//! Value types carried by data pins, and the layouts that type mirrors are checked against.

use serde::{Deserialize, Serialize};

use crate::NodeId;

/// A value's type, spelled per language on demand. These are same-target types (§5.9): widths and
/// layout are pinned by the build configuration, not a portable wire format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ValueType {
    Void,
    Bool,
    Int {
        bits: u8,
        signed: bool,
    },
    Float {
        bits: u8,
    },
    /// A pointer passed or returned as a value. Memory reached *through* a pointer parameter is a
    /// memory pin instead.
    Pointer {
        pointee: Box<ValueType>,
    },
    /// A struct, union, enum or typedef, linked to its type node when resolved.
    Named {
        name: String,
        def: Option<NodeId>,
    },
    /// Known only by its source spelling: syntax-only analysis, before name/type resolution.
    Spelled {
        c: String,
    },
    Unknown,
}

impl ValueType {
    pub const fn u32() -> Self {
        Self::Int {
            bits: 32,
            signed: false,
        }
    }

    pub const fn i32() -> Self {
        Self::Int {
            bits: 32,
            signed: true,
        }
    }

    pub fn pointer(pointee: ValueType) -> Self {
        Self::Pointer {
            pointee: Box::new(pointee),
        }
    }

    pub fn named(name: impl Into<String>, def: Option<NodeId>) -> Self {
        Self::Named {
            name: name.into(),
            def,
        }
    }

    /// C spelling, e.g. `uint32_t`, `struct ram *`.
    pub fn c(&self) -> String {
        match self {
            Self::Void => "void".into(),
            Self::Bool => "bool".into(),
            Self::Int { bits, signed } => format!("{}int{bits}_t", if *signed { "" } else { "u" }),
            Self::Float { bits: 32 } => "float".into(),
            Self::Float { .. } => "double".into(),
            Self::Pointer { pointee } => {
                let inner = pointee.c();
                if inner.ends_with('*') {
                    format!("{inner}*")
                } else {
                    format!("{inner} *")
                }
            }
            Self::Named { name, .. } => name.clone(),
            Self::Spelled { c } => c.clone(),
            Self::Unknown => "?".into(),
        }
    }

    /// Rust spelling at the current (C-shaped) ABI, e.g. `u32`, `*mut ram`.
    pub fn rust(&self) -> String {
        match self {
            Self::Void => "()".into(),
            Self::Bool => "bool".into(),
            Self::Int { bits, signed } => format!("{}{bits}", if *signed { 'i' } else { 'u' }),
            Self::Float { bits } => format!("f{bits}"),
            Self::Pointer { pointee } => format!("*mut {}", pointee.rust()),
            Self::Named { name, .. } => name
                .trim_start_matches("struct ")
                .trim_start_matches("union ")
                .trim_start_matches("enum ")
                .to_string(),
            Self::Spelled { c } => format!("/* {c} */"),
            Self::Unknown => "_".into(),
        }
    }

    /// Wires may join equal types, or anything with a not-yet-resolved side.
    pub fn compatible(&self, other: &ValueType) -> bool {
        self == other
            || matches!(self, Self::Unknown | Self::Spelled { .. })
            || matches!(other, Self::Unknown | Self::Spelled { .. })
    }
}

/// Size, alignment and field offsets of a struct/union/enum on the pinned target. Required where a
/// type crosses the mixed-language ABI; both sides assert it (§5.8–§5.9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeLayout {
    /// The type node.
    pub node: NodeId,
    pub size: u64,
    pub align: u64,
    pub fields: Vec<FieldLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldLayout {
    pub name: String,
    pub offset: u64,
    pub ty: ValueType,
}
