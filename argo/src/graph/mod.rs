//! Source-retaining syntax graphs. Containment is not an execution schedule.
//! No dataflow, resolved control targets, outlining, or interface safety is implied.

mod builder;
mod cases;
mod dispatch;
mod error;
mod expansion;
mod kinds;
mod model;
mod provenance;
mod regions;
mod sequence;
mod switch;
mod validation;

pub(crate) mod constructs;

pub use builder::build;
pub use error::GraphError;
pub use expansion::{ChildRegion, Expansion, NodeDraft, Unsupported};
pub use kinds::{ControlExitKind, GraphEdgeKind, GraphNodeKind};
pub use model::{Graph, GraphDiagnostic, GraphEdge, GraphNode, NodeId};
pub use provenance::{ControlExit, SourceSpan, SyntaxPart};
pub use sequence::DECLARATIONS;
