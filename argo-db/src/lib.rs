//! Argo's SpacetimeDB module (ARCHITECTURE.md §4, "SpacetimeDB integration").
//!
//! Public tables: `c_chunk` (the ingested C program, one row per chunk), `rust_chunk` (one row
//! per claimed chunk: placeholder, result or failure) and `model_exchange` (the recorded model
//! traffic, see `exchange`). The private `config` row holds
//! only authorization state: the owner identity, which repository commit is ingested and the
//! database-wide fencing counter. Reducers validate identity, fencing and bounds; they never
//! read files, call models or compile anything.

pub mod claim;
pub mod exchange;
pub mod ingest;
pub mod limits;
pub mod owner;
pub mod rules;
pub mod submit;
pub mod tables;
