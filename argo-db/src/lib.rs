//! Argo's SpacetimeDB module (ARCHITECTURE.md §4, "SpacetimeDB integration").
//!
//! Two public tables: `c_chunk` (the ingested C program, one row per chunk) and `rust_chunk`
//! (one row per claimed chunk: placeholder, result or failure). The private `config` row holds
//! only authorization state: the owner identity, which repository commit is ingested and the
//! database-wide fencing counter. Reducers validate identity, fencing and bounds; they never
//! read files, call models or compile anything.

pub mod claim;
pub mod ingest;
pub mod limits;
pub mod owner;
pub mod rules;
pub mod submit;
pub mod tables;
