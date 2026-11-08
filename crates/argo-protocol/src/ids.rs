//! Typed identities. S assigns every numeric ID; never trust a worker-generated one for ownership.
//!
//! Numeric node and edge IDs are compact handles **within one graph revision**. Identity that
//! survives across revisions is the node's [`LogicalKey`] plus its configured instance, and a
//! pin's identity is its node plus its [`PinKey`](crate::pin::PinKey).

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! numeric_id {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}#{}", stringify!($name), self.0)
            }
        }
    )*};
}

macro_rules! string_id {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.into())
            }
        }
    )*};
}

numeric_id! {
    ProjectId,
    RevisionId,
    /// A node within one revision.
    NodeId,
    /// An edge within one revision.
    EdgeId,
    /// A source file within a sealed source bundle.
    FileId,
    /// One translation-unit/template configuration of shared source (§4, §5.5).
    InstanceId,
    /// A worker known to S. Display ordinal; authentication identity is the transport's concern.
    WorkerId,
    JobId,
    AttemptId,
    ImplementationId,
    ReportId,
}

string_id! {
    /// Stable across revisions: derived from the configured instance and structural position,
    /// never from a name or collision ordinal alone (§4).
    LogicalKey,
    /// The template that produced a node, e.g. `c.if_statement`.
    TemplateId,
    /// Lowercase hex SHA-256.
    ContentHash,
}
