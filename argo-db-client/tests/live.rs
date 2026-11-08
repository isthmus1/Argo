//! End to end against a running host: ownership granted by the publisher, ingest through the
//! SDK, rebuild from a second client's subscription cache, claim, and get stale, foreign and
//! duplicate writes rejected, including after a requeue or a reset and re-ingest hands the same
//! chunk to the same identity again. Failed chunks stay out of `claim_next` until requeued.
//! Lease expiry (10 minutes) is covered by the module's pure `claimable` rule, not here.
//!
//! Needs a database freshly published by this machine's `spacetime` CLI identity (which `init`
//! records as the publisher):
//! `ARGO_DB=ws://127.0.0.1:3101 ARGO_MODULE=argo cargo test -p argo-db-client --test live -- --ignored`

use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use argo_db_client::bindings::*;
use argo_db_client::{Origin, batches, rebuild, to_rows};
use argo_protocol::{Snapshot, fixture};
use spacetimedb_sdk::{DbContext, Identity, Table};

const WAIT: Duration = Duration::from_secs(20);

fn uri() -> String {
    std::env::var("ARGO_DB").expect("ARGO_DB=ws://host:port")
}

fn db() -> String {
    std::env::var("ARGO_MODULE").unwrap_or_else(|_| "argo".into())
}

fn connect() -> (DbConnection, Identity) {
    let (uri, db) = (uri(), db());
    let (tx, rx) = mpsc::channel();
    let conn = DbConnection::builder()
        .with_uri(uri)
        .with_database_name(db)
        .on_connect(move |_, id, _| tx.send(id).unwrap())
        .build()
        .unwrap();
    conn.run_threaded();
    let id = rx.recv_timeout(WAIT).unwrap();
    let (tx, rx) = mpsc::channel();
    conn.subscription_builder()
        .on_applied(move |_| tx.send(()).unwrap())
        .subscribe_to_all_tables();
    rx.recv_timeout(WAIT).unwrap();
    (conn, id)
}

/// Have the publisher (the CLI identity) make `id` the owner, as `argo-server` does.
fn grant(id: Identity) {
    let server = uri().replacen("ws", "http", 1);
    let arg = format!(r#"{{"__identity__": "0x{}"}}"#, id.to_hex());
    let out = Command::new("spacetime")
        .args(["call", "--server", &server, &db(), "grant_ownership", &arg])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "spacetime call failed: {stderr}");
}

type Done = Box<
    dyn FnOnce(
            &ReducerEventContext,
            Result<Result<(), String>, spacetimedb_sdk::__codegen::InternalError>,
        ) + Send,
>;

/// Invoke a reducer and wait for its outcome.
fn call(invoke: impl FnOnce(Done)) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    invoke(Box::new(move |_, r| {
        tx.send(r.map_err(|e| e.to_string()).and_then(|r| r))
            .unwrap()
    }));
    rx.recv_timeout(WAIT).unwrap()
}

fn ingest(owner: &DbConnection, rows: &[CChunk], commit: &str) {
    let begin = |locator: &str, f| {
        let r = &owner.reducers;
        r.ingest_begin_then(locator.into(), commit.into(), f)
            .unwrap()
    };
    assert!(call(|f| begin("", f)).is_err(), "empty locator");
    call(|f| begin("fixture", f)).unwrap();
    for batch in batches(rows.to_vec()).unwrap() {
        call(|f| owner.reducers.ingest_rows_then(batch, f).unwrap()).unwrap();
    }
    let total = rows.len() as u64;
    call(|f| owner.reducers.ingest_finish_then(total, f).unwrap()).unwrap();
}

/// Claim and wait for this identity's placeholder with a generation past `after`.
fn claim(worker: &DbConnection, me: Identity, after: u32) -> RustChunk {
    call(|f| worker.reducers.claim_next_then(f).unwrap()).unwrap();
    let start = Instant::now();
    loop {
        let table = worker.db.rust_chunk();
        if let Some(r) = table
            .iter()
            .find(|r| r.worker == me && r.generation > after)
        {
            return r;
        }
        assert!(start.elapsed() < WAIT, "claim never reached the cache");
        std::thread::yield_now();
    }
}

fn submit(worker: &DbConnection, chunk: u64, generation: u32, model: &str) -> Result<(), String> {
    let (source, model, log) = ("x".to_string(), model.to_string(), String::new());
    let unvalidated = Validation::Unvalidated;
    call(|f| {
        let r = &worker.reducers;
        r.submit_rust_then(chunk, generation, source, String::new(), model, 1, unvalidated, log, f)
            .unwrap()
    })
}

#[test]
#[ignore = "needs a running SpacetimeDB host with argo-db freshly published"]
fn ingest_rebuild_claim_and_fence() {
    let snap = fixture::snapshot();
    let origin = Origin {
        locator: "fixture".into(),
        commit: Some("c0ffee".into()),
        units: vec![],
        flags: vec![],
    };
    let rows = to_rows(&snap, &origin, &|_, _| None).unwrap();

    let (owner, owner_id) = connect();
    let (worker, me) = connect();
    let (other, _) = connect();
    let begin = |c: &DbConnection| {
        let r = &c.reducers;
        call(|f| r.ingest_begin_then("x".into(), "y".into(), f).unwrap())
    };
    assert!(begin(&owner).is_err(), "no ownership before the grant");
    let take = call(|f| worker.reducers.grant_ownership_then(me, f).unwrap());
    assert!(take.unwrap_err().contains("publisher or the owner"));
    grant(owner_id);
    ingest(&owner, &rows, "c0ffee");
    assert!(begin(&worker).is_err(), "non-owner ingest_begin");
    let batch = vec![rows[0].clone()];
    let foreign_rows = call(|f| worker.reducers.ingest_rows_then(batch, f).unwrap());
    assert!(foreign_rows.unwrap_err().contains("only the owner"));

    let chunks: Vec<CChunk> = worker.db.c_chunk().iter().collect();
    let structure = rebuild(&chunks).unwrap();
    assert_eq!(
        structure.snapshot,
        Snapshot {
            workers: vec![],
            ..snap
        }
    );
    assert!(
        call(|f| worker.reducers.ingest_reset_then(f).unwrap()).is_err(),
        "non-owner reset"
    );

    let first = claim(&worker, me, 0);
    let leaf = chunks.iter().find(|c| c.id == first.chunk).unwrap();
    assert!(leaf.translatable && leaf.descendants == 0, "leaves first");
    let live = structure.live([&first]);
    assert!(
        live.status(argo_protocol::NodeId(leaf.node.unwrap()))
            .unwrap()
            .claim
            .is_some()
    );

    let stale = submit(&worker, first.chunk, first.generation + 1, "m");
    assert!(stale.unwrap_err().contains("stale generation"));
    let long_model = submit(&worker, first.chunk, first.generation, &"m".repeat(257));
    assert!(long_model.unwrap_err().contains("model"));
    let foreign = call(|f| {
        owner
            .reducers
            .heartbeat_then(first.chunk, first.generation, f)
            .unwrap()
    });
    assert!(foreign.unwrap_err().contains("another worker"));

    // Requeue, then the same identity is handed the same chunk again (leaves first): the old
    // attempt's generation must not match the new claim.
    call(|f| owner.reducers.requeue_then(first.chunk, f).unwrap()).unwrap();
    let again = claim(&worker, me, first.generation);
    assert_eq!(again.chunk, first.chunk);
    assert!(submit(&worker, first.chunk, first.generation, "m").is_err());

    // Reset and ingest another commit: chunk ids repeat, generations don't.
    call(|f| owner.reducers.ingest_reset_then(f).unwrap()).unwrap();
    ingest(&owner, &rows, "beef");
    let fresh = claim(&worker, me, again.generation);
    assert_eq!(fresh.chunk, first.chunk);
    assert!(submit(&worker, fresh.chunk, again.generation, "m").is_err());
    submit(&worker, fresh.chunk, fresh.generation, "m").unwrap();
    let duplicate = submit(&worker, fresh.chunk, fresh.generation, "m");
    assert!(duplicate.unwrap_err().contains("Done, not Claimed"));

    // Another identity can't finish someone else's claim; a failure is terminal.
    let second = claim(&worker, me, fresh.generation);
    let foreign = submit(&other, second.chunk, second.generation, "m");
    assert!(foreign.unwrap_err().contains("another worker"));
    let fail = |c: &DbConnection| {
        let r = &c.reducers;
        call(|f| {
            r.fail_chunk_then(second.chunk, second.generation, 3, "log".into(), f)
                .unwrap()
        })
    };
    assert!(fail(&other).unwrap_err().contains("another worker"));
    fail(&worker).unwrap();
    assert!(fail(&worker).unwrap_err().contains("Failed, not Claimed"));

    // Drain the rest: neither the Done nor the Failed chunk is handed out again.
    let mut last = second.generation;
    while call(|f| worker.reducers.claim_next_then(f).unwrap()).is_ok() {
        let mine = |r: &RustChunk| r.worker == me && r.generation > last;
        let start = Instant::now();
        let next = loop {
            if let Some(r) = worker.db.rust_chunk().iter().find(mine) {
                break r;
            }
            assert!(start.elapsed() < WAIT, "claim never reached the cache");
            std::thread::yield_now();
        };
        assert!(
            ![fresh.chunk, second.chunk].contains(&next.chunk),
            "re-claimed"
        );
        last = next.generation;
    }
    let none = call(|f| worker.reducers.claim_next_then(f).unwrap());
    assert!(none.unwrap_err().contains("no chunk is available"));

    // Only the owner requeues; a requeued failure is claimable again.
    let requeue = |c: &DbConnection| call(|f| c.reducers.requeue_then(second.chunk, f).unwrap());
    assert!(requeue(&worker).unwrap_err().contains("only the owner"));
    requeue(&owner).unwrap();
    assert_eq!(claim(&worker, me, last).chunk, second.chunk);
    for c in [owner, worker, other] {
        c.disconnect().unwrap();
    }
}
