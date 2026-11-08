//! The owner: the one identity allowed to ingest and requeue. Stored in the private config row,
//! with the database-wide fencing counter (an authorization token source, not domain data).
//!
//! Ownership is granted, never taken: `init` records the publishing identity (the `spacetime`
//! CLI identity `argo-server` publishes with), and only it or the current owner may name a new
//! owner. So no other client can grab a fresh database before the server does, and a server
//! restarted on a persisted data dir with a new SDK identity can grant itself ownership again.

use spacetimedb::{Identity, ReducerContext, Table, reducer, table};

/// Private: authorization and ingest configuration only, never domain data.
#[table(accessor = config)]
pub struct Config {
    /// Always [`CONFIG`]: a single row.
    #[primary_key]
    pub key: u8,
    /// Who published the module; may always reassign ownership.
    pub publisher: Identity,
    pub owner: Identity,
    pub locator: String,
    pub commit: String,
    /// `ingest_finish` checked the row count; claims may start.
    pub sealed: bool,
    /// The last fencing generation any claim was given (`rules::next_fence`). Kept across
    /// `ingest_reset`, so no generation is ever handed out twice.
    pub fence: u32,
}

/// Primary key of the single config row.
pub const CONFIG: u8 = 0;

pub fn config(ctx: &ReducerContext) -> Option<Config> {
    ctx.db.config().key().find(CONFIG)
}

/// The config row, if the caller is its owner.
pub fn owner(ctx: &ReducerContext) -> Result<Config, String> {
    match config(ctx) {
        Some(c) if c.owner == ctx.sender() => Ok(c),
        _ => Err("only the owner may do this".into()),
    }
}

/// On first publish: the publisher owns the database until it grants ownership elsewhere.
#[reducer(init)]
pub fn init(ctx: &ReducerContext) {
    ctx.db.config().insert(Config {
        key: CONFIG,
        publisher: ctx.sender(),
        owner: ctx.sender(),
        locator: String::new(),
        commit: String::new(),
        sealed: false,
        fence: 0,
    });
}

/// Make `owner` the owner. `argo-server` connects with the SDK, then has the CLI identity it
/// published with call this for the SDK identity (`spacetime call`).
#[reducer]
pub fn grant_ownership(ctx: &ReducerContext, owner: Identity) -> Result<(), String> {
    let c = config(ctx).ok_or("the module was not initialized")?;
    if ctx.sender() != c.publisher && ctx.sender() != c.owner {
        return Err("only the publisher or the owner may grant ownership".into());
    }
    ctx.db.config().key().update(Config { owner, ..c });
    Ok(())
}
