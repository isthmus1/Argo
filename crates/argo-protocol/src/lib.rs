//! Argo protocol v1: the shared meaning of graph and migration records.
//!
//! These records are framework-independent (ARCHITECTURE.md §3–§4). SpacetimeDB bindings are the
//! transport; this crate is what the rows mean, plus the checks and fixtures every part agrees on.
//!
//! Two kinds of record, kept apart on purpose:
//!
//! - **Structure** ([`Graph`]): nodes, pins, edges, type layouts and source references for one
//!   graph revision. W publishes it, S freezes it, and [`canonical_hash`] covers exactly this.
//! - **Status** ([`Snapshot`] minus its graph): migration state, worker claims, candidates,
//!   reports and annotations. These change while a revision stays frozen and are never hashed.
//!
//! Node positions, viewport and selection belong to the GUI and have no record here.
//!
//! # Pins
//!
//! Every pin is in one of two families that never mix ([`Family`]): **control** (exec) pins carry
//! execution order, **data** pins carry values, state handles and memory handles. A node never
//! reads anything it doesn't have a pin for, and wires only join siblings: a value enters a
//! region through the region's own input pin and reappears inside on its [`NodeKind::GroupInput`].
//! See [`pin`] for the key conventions and [`validate`] for the rules S enforces at freeze.

pub mod builder;
pub mod edge;
pub mod effect;
pub mod fixture;
pub mod hash;
pub mod ids;
pub mod index;
pub mod kind;
pub mod node;
pub mod pin;
pub mod revision;
pub mod snapshot;
pub mod source;
pub mod status;
pub mod types;
pub mod validate;
pub mod view;
pub mod work;

pub use builder::GraphBuilder;
pub use edge::{Edge, Relation};
pub use effect::{Access, Effect};
pub use hash::canonical_hash;
pub use ids::*;
pub use index::GraphIndex;
pub use kind::{DispatchKind, LoopKind, NodeFamily, NodeKind, TypeKind};
pub use node::Node;
pub use pin::{Direction, Family, Flow, Pin, PinKind, PinOrigin, PinRef, PinSpec};
pub use revision::{Analysis, Graph, GraphRecord, GraphRevision, RevisionState};
pub use snapshot::{Snapshot, Update};
pub use source::{FileRef, Provenance, SourceFile, SourceRange, SyntaxPart, SyntaxRole};
pub use status::{
    Candidate, Claim, ClaimPhase, EdgeStatus, Failure, Language, NodeStatus, Verdict, Worker,
};
pub use types::{FieldLayout, TypeLayout, ValueType};
pub use validate::{Violation, validate};
pub use work::{
    Abi, Annotation, AnnotationKind, Attempt, Author, Counterexample, Implementation, Job,
    JobState, Outcome, Stage, Tier, ValidationReport,
};

/// Bumped on any incompatible change to a record's meaning or encoding.
pub const SCHEMA_VERSION: u32 = 1;
