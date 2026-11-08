//! The worker's parts as a library, so the compile/link/C-test pipeline can implement
//! [`validate::Validator`] and drive [`work::Worker`] with it; `main.rs` wires the defaults.

pub mod attempt;
pub mod checkout;
pub mod claim;
pub mod db;
pub mod fence;
pub mod log;
pub mod model;
pub mod prompt;
pub mod resend;
pub mod select;
pub mod shape;
pub mod validate;
pub mod work;
pub mod workdir;
