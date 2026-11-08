//! The client side of `argo-db`: generated SpacetimeDB bindings plus the one shared mapping
//! between `argo_protocol` records and database rows.
//!
//! - [`to_rows`]: a [`Snapshot`](argo_protocol::Snapshot) as `c_chunk` rows (server ingest),
//!   each located both in the ingested text and, through [`Places`], in the original file.
//! - [`rebuild`] and [`Structure::live`]: rows back into a snapshot whose node status reflects the
//!   `rust_chunk` rows (GUI). The GUI needs nothing but these rows.
//!
//! Every protocol record is owned by exactly one chunk, so the union of rows reconstructs the
//! graph exactly: a node by its nearest enclosing chunk head, a pin by its node, an edge by the
//! first node of its relation, a layout by its type node. Records keep their original position,
//! so record order survives too.

#[rustfmt::skip]
pub mod bindings;
mod error;
/// The module's own limits file, compiled here too so both sides use the same numbers.
#[path = "../../argo-db/src/limits.rs"]
mod limits;
mod partition;
mod rebuild;
mod records;
mod rows;
mod status;

pub use bindings::{
    CChunk, ChunkKind, DbConnection, RemoteDbContext, RustChunk, RustStatus, Validation,
};
pub use error::Error;
pub use limits::{
    LEASE_MICROS, MAX_BATCH_BYTES, MAX_BATCH_ROWS, MAX_LOG_BYTES, MAX_MODEL_BYTES, MAX_RUST_BYTES,
    NONE_FREE, NOT_SEALED, ROW_OVERHEAD,
};
pub use rebuild::{Structure, origin, rebuild};
pub use records::{Origin, Unit};
pub use rows::{Places, ROOT, SEGMENT_BYTES, batches, row_bytes, to_rows};
