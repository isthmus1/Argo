//! `--replay FROM`: play the run recorded in database `FROM` into this server's (fresh) database,
//! faster by `--speed`, for the GUI to watch as if live.
//!
//! The recording is what `FROM` holds: its `c_chunk` rows (the program, copied as ingested), its
//! `rust_chunk` rows (each claim's worker, when it was claimed and when it finished, and how) and
//! its `model_exchange` rows (every model request and reply, see `argo-db`'s `exchange`). Each
//! claim, result and exchange is written at its recorded time, scaled, through the owner-only
//! `replay_rust` and `record_exchange` reducers. Nothing is translated, compiled or asked of a
//! model, so a replay shows exactly what was recorded, every time.
//!
//! A `rust_chunk` row keeps only its chunk's latest claim, and a released claim (a worker out of
//! attempts gives the chunk back) leaves no row at all. Those earlier claims are rebuilt from
//! their exchanges, which name the chunk and generation: claimed at the first request, released
//! (`requeue`) after the last reply, by the worker identity the same proxy name had on a claim
//! that kept its row. A lapsed claim made no requests through the proxy, so it doesn't replay.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use argo_db_client::bindings::{
    CChunkTableAccess, ModelExchangeTableAccess, RustChunkTableAccess, ingest_begin, ingest_finish,
    ingest_rows, record_exchange, replay_rust, requeue,
};
use argo_db_client::{CChunk, ModelExchange, ROOT, RustChunk, RustStatus, Validation, batches};
use spacetimedb_sdk::{DbContext, Identity, Table, Timestamp};
use tokio::time::{Instant, sleep_until};

use crate::conn::{Conn, reply};

/// One recorded moment.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Claim(RustChunk),
    /// A `Done` or `Failed` row, with its C row's `c_call`.
    Finish(RustChunk, String),
    Exchange(ModelExchange),
    /// A claim given back without a result: its row goes.
    Release(u64),
}

impl Event {
    /// At equal times a claim comes before its exchanges, and they before its result.
    fn rank(&self) -> u8 {
        match self {
            Event::Claim(_) => 0,
            Event::Exchange(_) => 1,
            Event::Finish(..) | Event::Release(_) => 2,
        }
    }
}

/// What `FROM` recorded.
struct Recording {
    chunks: Vec<CChunk>,
    rust: Vec<RustChunk>,
    exchanges: Vec<ModelExchange>,
}

/// Copy `from`'s program into `to` (owned, freshly published), then play its run at `speed`.
pub async fn replay(to: &Conn, from: &Conn, name: &str, speed: f64) -> Result<(), String> {
    let recording = read(from)
        .await
        .map_err(|e| format!("reading {name}: {e}"))?;
    ingest(to, &recording.chunks).await?;
    let events = timeline(&recording);
    let span = events
        .last()
        .zip(events.first())
        .map_or(0, |(l, f)| l.0 - f.0);
    eprintln!(
        "argo-server: replaying {name}: {} claims and {} model exchanges over {} at {speed}x, \
         about {}",
        events
            .iter()
            .filter(|(_, e)| matches!(e, Event::Claim(_)))
            .count(),
        recording.exchanges.len(),
        minutes(span),
        minutes((span as f64 / speed) as i64),
    );
    play(to, events, speed).await?;
    eprintln!("argo-server: replay of {name} finished");
    Ok(())
}

/// Everything `from` holds, through one subscription.
async fn read(from: &Conn) -> Result<Recording, String> {
    let (applied, rx) = reply();
    let failed = applied.clone();
    let _sub = from
        .db
        .subscription_builder()
        .on_applied(move |_| applied.send(Ok(())))
        .on_error(move |_, e| failed.send(Err(e.to_string())))
        .subscribe([
            "SELECT * FROM c_chunk",
            "SELECT * FROM rust_chunk",
            "SELECT * FROM model_exchange",
        ]);
    from.or_lost(rx).await??;
    let db = &from.db.db;
    let mut chunks: Vec<_> = db.c_chunk().iter().collect();
    chunks.sort_by_key(|c| c.id);
    if chunks.is_empty() {
        return Err("it holds no program".into());
    }
    Ok(Recording {
        chunks,
        rust: db.rust_chunk().iter().collect(),
        exchanges: db.model_exchange().iter().collect(),
    })
}

/// The recorded program as ingested (no translations yet), under the recording's origin.
async fn ingest(to: &Conn, chunks: &[CChunk]) -> Result<(), String> {
    let root = chunks.iter().find(|c| c.id == ROOT);
    let origin = argo_db_client::origin(root, chunks.len() as u64).map_err(|e| e.to_string())?;
    let reducers = &to.db.reducers;
    let (locator, commit) = (origin.locator, origin.commit.unwrap_or_default());
    to.call(|f| reducers.ingest_begin_then(locator, commit, f))
        .await
        .map_err(|e| format!("ingest_begin: {e}"))?;
    let rows = chunks.iter().map(|c| CChunk {
        rust: String::new(),
        c_call: String::new(),
        ..c.clone()
    });
    for batch in batches(rows.collect()).map_err(|e| e.to_string())? {
        to.call(|f| reducers.ingest_rows_then(batch, f))
            .await
            .map_err(|e| format!("ingest_rows: {e}"))?;
    }
    to.call(|f| reducers.ingest_finish_then(chunks.len() as u64, f))
        .await
        .map_err(|e| format!("ingest_finish: {e}"))
}

/// Every claim, exchange and result at its recorded time (microseconds), in order.
fn timeline(r: &Recording) -> Vec<(i64, Event)> {
    let c_call: HashMap<u64, &str> = r.chunks.iter().map(|c| (c.id, c.c_call.as_str())).collect();
    let mut events = Vec::new();
    for row in &r.rust {
        let claimed = RustChunk {
            status: RustStatus::Claimed,
            attempts: 0,
            model: String::new(),
            source: String::new(),
            validation: Validation::Unvalidated,
            log: String::new(),
            ..row.clone()
        };
        events.push((
            row.claimed_at.to_micros_since_unix_epoch(),
            Event::Claim(claimed),
        ));
        if row.status != RustStatus::Claimed {
            let call = c_call
                .get(&row.chunk)
                .copied()
                .unwrap_or_default()
                .to_string();
            let at = row.updated_at.to_micros_since_unix_epoch();
            events.push((at, Event::Finish(row.clone(), call)));
        }
    }
    events.extend(released(r));
    for x in &r.exchanges {
        events.push((
            x.answered_at.to_micros_since_unix_epoch(),
            Event::Exchange(x.clone()),
        ));
    }
    events.sort_by(|a, b| match a.0.cmp(&b.0) {
        Ordering::Equal => a.1.rank().cmp(&b.1.rank()),
        other => other,
    });
    events
}

/// The claims that left no `rust_chunk` row, from their exchanges: a claim at the first request
/// and a release after the last reply.
fn released(r: &Recording) -> Vec<(i64, Event)> {
    let kept: HashMap<(u64, u32), Identity> = r
        .rust
        .iter()
        .map(|row| ((row.chunk, row.generation), row.worker))
        .collect();
    // Per claim: first request, last reply, the proxy's worker name.
    let mut claims: BTreeMap<(u64, u32), (i64, i64, &str)> = BTreeMap::new();
    let mut identity: HashMap<&str, Identity> = HashMap::new();
    for x in &r.exchanges {
        let (Some(chunk), Some(generation)) = (x.chunk, x.generation) else {
            continue;
        };
        if let Some(worker) = kept.get(&(chunk, generation)) {
            identity.insert(&x.worker, *worker);
            continue;
        }
        let (sent, answered) = (micros(x.sent_at), micros(x.answered_at));
        let span = claims
            .entry((chunk, generation))
            .or_insert((sent, answered, &x.worker));
        span.0 = span.0.min(sent);
        span.1 = span.1.max(answered);
    }
    let mut events = Vec::new();
    for ((chunk, generation), (first, last, name)) in claims {
        let row = RustChunk {
            chunk,
            status: RustStatus::Claimed,
            worker: identity.get(name).copied().unwrap_or(Identity::ZERO),
            generation,
            claimed_at: Timestamp::from_micros_since_unix_epoch(first),
            updated_at: Timestamp::from_micros_since_unix_epoch(first),
            attempts: 0,
            model: String::new(),
            source: String::new(),
            validation: Validation::Unvalidated,
            log: String::new(),
        };
        events.push((first, Event::Claim(row)));
        events.push((last, Event::Release(chunk)));
    }
    events
}

fn micros(t: Timestamp) -> i64 {
    t.to_micros_since_unix_epoch()
}

/// Write each event when its scaled time comes, waiting for each to be applied.
async fn play(to: &Conn, events: Vec<(i64, Event)>, speed: f64) -> Result<(), String> {
    let Some(&(first, _)) = events.first() else {
        return Ok(());
    };
    let start = Instant::now();
    let total = events.len();
    let reducers = &to.db.reducers;
    for (n, (at, event)) in events.into_iter().enumerate() {
        sleep_until(start + scaled(at - first, speed)).await;
        let what = match event {
            Event::Claim(row) => {
                to.call(|f| reducers.replay_rust_then(row, String::new(), f))
                    .await
            }
            Event::Finish(row, call) => to.call(|f| reducers.replay_rust_then(row, call, f)).await,
            Event::Exchange(x) => to.call(|f| reducers.record_exchange_then(x, f)).await,
            Event::Release(chunk) => to.call(|f| reducers.requeue_then(chunk, f)).await,
        };
        what.map_err(|e| format!("replay event {n}: {e}"))?;
        if (n + 1) % (total / 10).max(1) == 0 {
            eprintln!("argo-server: replayed {}/{total}", n + 1);
        }
    }
    Ok(())
}

/// `micros` of recorded time at `speed`.
fn scaled(micros: i64, speed: f64) -> Duration {
    Duration::from_secs_f64(micros.max(0) as f64 / 1e6 / speed)
}

fn minutes(micros: i64) -> String {
    let secs = micros.max(0) / 1_000_000;
    format!("{}m{:02}s", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use spacetimedb_sdk::{Identity, Timestamp};

    use super::*;

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_micros_since_unix_epoch(secs * 1_000_000)
    }

    fn rust(chunk: u64, status: RustStatus, claimed: i64, updated: i64) -> RustChunk {
        RustChunk {
            chunk,
            status,
            worker: Identity::ZERO,
            generation: 9,
            claimed_at: at(claimed),
            updated_at: at(updated),
            attempts: 3,
            model: "m".into(),
            source: "pub fn f() {}".into(),
            validation: Validation::Unvalidated,
            log: "log".into(),
        }
    }

    fn exchange(answered: i64) -> ModelExchange {
        ModelExchange {
            id: 1,
            worker: "w1".into(),
            chunk: Some(1),
            generation: Some(9),
            sent_at: at(answered - 1),
            answered_at: at(answered),
            status: 200,
            prompt_tokens: 1,
            completion_tokens: 1,
            cost: 0.0,
            prefix: 0,
            request: "{}".into(),
            response: "{}".into(),
        }
    }

    #[test]
    fn claims_exchanges_and_results_play_in_recorded_order() {
        let mut chunk = CChunk {
            id: 1,
            node: None,
            parent: None,
            name: "f".into(),
            kind: argo_db_client::ChunkKind::Function,
            translatable: true,
            depth: 0,
            descendants: 0,
            path: String::new(),
            line_start: 0,
            line_end: 0,
            ingested: String::new(),
            byte_lo: 0,
            byte_hi: 0,
            source: String::new(),
            records: String::new(),
            rust: "pub fn f() {}".into(),
            c_call: "f_rs();".into(),
        };
        let other = CChunk {
            id: 2,
            ..chunk.clone()
        };
        chunk.c_call = "f_rs();".into();
        let r = Recording {
            chunks: vec![chunk, other],
            rust: vec![
                rust(1, RustStatus::Done, 10, 30),
                rust(2, RustStatus::Claimed, 20, 25),
            ],
            exchanges: vec![exchange(30), exchange(15)],
        };
        let events = timeline(&r);
        let shape: Vec<_> = events
            .iter()
            .map(|(t, e)| (t / 1_000_000, e.rank()))
            .collect();
        assert_eq!(shape, [(10, 0), (15, 1), (20, 0), (30, 1), (30, 2)]);
        let Event::Claim(claim) = &events[0].1 else {
            panic!()
        };
        assert_eq!(
            (claim.status, claim.source.as_str(), claim.attempts),
            (RustStatus::Claimed, "", 0)
        );
        let Event::Finish(done, call) = &events[4].1 else {
            panic!()
        };
        assert_eq!(
            (done.status, done.attempts, call.as_str()),
            (RustStatus::Done, 3, "f_rs();")
        );
        assert!(
            !events
                .iter()
                .any(|(_, e)| matches!(e, Event::Finish(r, _) if r.chunk == 2)),
            "a claim still open at the end of the recording only starts"
        );
    }

    #[test]
    fn released_claims_come_back_from_their_exchanges() {
        let worker = Identity::from_byte_array([7; 32]);
        let kept = RustChunk {
            worker,
            ..rust(1, RustStatus::Done, 50, 60)
        };
        let on = |chunk, generation, answered, name: &str| ModelExchange {
            chunk: Some(chunk),
            generation: Some(generation),
            worker: name.into(),
            ..exchange(answered)
        };
        let r = Recording {
            chunks: vec![],
            rust: vec![kept],
            // Chunk 1 was released at generation 4 before its kept claim at 9; chunk 3 was
            // released and never claimed again, by a worker with no kept row.
            exchanges: vec![
                on(1, 4, 12, "w1"),
                on(1, 4, 20, "w1"),
                on(1, 9, 55, "w1"),
                on(3, 5, 30, "w2"),
            ],
        };
        let events = timeline(&r);
        let shape: Vec<_> = events
            .iter()
            .map(|(t, e)| match e {
                Event::Claim(row) => (
                    t / 1_000_000,
                    format!("claim {} {}", row.chunk, row.worker == worker),
                ),
                Event::Release(c) => (t / 1_000_000, format!("release {c}")),
                Event::Exchange(x) => (t / 1_000_000, format!("exchange {}", x.chunk.unwrap())),
                Event::Finish(row, _) => (t / 1_000_000, format!("finish {}", row.chunk)),
            })
            .collect();
        let expect = [
            (11, "claim 1 true"),
            (12, "exchange 1"),
            (20, "exchange 1"),
            (20, "release 1"),
            (29, "claim 3 false"),
            (30, "exchange 3"),
            (30, "release 3"),
            (50, "claim 1 true"),
            (55, "exchange 1"),
            (60, "finish 1"),
        ];
        let expect: Vec<_> = expect.iter().map(|(t, s)| (*t, s.to_string())).collect();
        assert_eq!(shape, expect);
    }

    #[test]
    fn time_is_scaled_and_spans_read_as_minutes() {
        assert_eq!(scaled(100_000_000, 100.0), Duration::from_secs(1));
        assert_eq!(scaled(-5, 100.0), Duration::ZERO);
        assert_eq!(minutes(3_723_000_000), "62m03s");
    }
}
