//! The feed against a scripted cache: each connection is followed on its own thread, as
//! `session` does, while the test plays its callbacks.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use argo_db_client::{CChunk, RustChunk, RustStatus, Validation};

use super::{Cache, Feed, Mark};
use crate::db::Msg;
use crate::db::tests::{chunks, rust};

struct Rows(Mutex<(Vec<CChunk>, Vec<RustChunk>)>);

impl Cache for Rows {
    fn chunks(&self) -> Vec<CChunk> {
        self.0.lock().unwrap().0.clone()
    }

    fn rust(&self) -> Vec<RustChunk> {
        self.0.lock().unwrap().1.clone()
    }
}

/// Follow one connection while `script` plays its marks (ending with `Lost`).
fn connection(
    mut feed: Feed,
    rows: &Arc<Rows>,
    script: impl FnOnce(&Sender<Mark>),
) -> (Feed, String, bool) {
    let (marks_tx, marks) = channel();
    let cache = Arc::clone(rows);
    let follower = std::thread::spawn(move || {
        let (why, applied) = feed.follow(&*cache, &marks);
        (feed, why, applied)
    });
    script(&marks_tx);
    follower.join().unwrap()
}

fn next(rx: &Receiver<Msg>) -> Option<Msg> {
    rx.recv_timeout(Duration::from_millis(500)).ok()
}

fn structure(msg: Option<Msg>) -> u64 {
    match msg {
        Some(Msg::Rows { structure, .. }) => structure,
        _ => panic!("expected the program's rows"),
    }
}

#[test]
fn bursts_rebuild_after_the_subscription_applies_and_structure_only_when_chunks_change() {
    let rows = Arc::new(Rows(Mutex::new((chunks(), vec![]))));
    let (tx, rx) = channel();
    let (_, why, applied) = connection(Feed::new(tx), &rows, |marks| {
        marks.send(Mark::Structure).unwrap();
        assert!(
            next(&rx).is_none(),
            "nothing is read before the subscription applies"
        );
        marks.send(Mark::Applied).unwrap();
        assert_eq!(structure(next(&rx)), 1);

        let step = rust(
            &rows.chunks(),
            "step",
            RustStatus::Claimed,
            Validation::Unvalidated,
        );
        rows.0.lock().unwrap().1.push(step);
        marks.send(Mark::Status).unwrap();
        assert_eq!(
            structure(next(&rx)),
            1,
            "a status change keeps the structure"
        );
        marks.send(Mark::Structure).unwrap();
        assert_eq!(
            structure(next(&rx)),
            1,
            "unchanged chunks keep the structure"
        );

        let last = rows.0.lock().unwrap().0.pop().unwrap();
        marks.send(Mark::Structure).unwrap();
        assert!(
            matches!(next(&rx), Some(Msg::Waiting(_))),
            "an incomplete ingest shows no program"
        );
        rows.0.lock().unwrap().0.push(last);
        marks.send(Mark::Structure).unwrap();
        assert_eq!(structure(next(&rx)), 3, "a new structure");
        marks.send(Mark::Lost("host stopped".into())).unwrap();
    });
    assert_eq!((why.as_str(), applied), ("host stopped", true));
}

#[test]
fn a_reconnect_keeps_the_program_and_resumes_as_a_status_update() {
    let rows = Arc::new(Rows(Mutex::new((chunks(), vec![]))));
    let (tx, rx) = channel();
    let feed = Feed::new(tx);
    assert!(feed.lost("cannot connect to ws://127.0.0.1:1"));
    assert!(matches!(next(&rx), Some(Msg::Online(false))));
    assert!(
        matches!(next(&rx), Some(Msg::Waiting(w)) if w.contains("cannot connect")),
        "an empty canvas says why"
    );

    let (feed, ..) = connection(feed, &rows, |marks| {
        marks.send(Mark::Applied).unwrap();
        assert_eq!(structure(next(&rx)), 1);
        marks.send(Mark::Lost("disconnected".into())).unwrap();
    });
    assert!(feed.lost("disconnected"));
    assert!(matches!(next(&rx), Some(Msg::Online(false))));
    assert!(next(&rx).is_none(), "the program stays on screen, offline");

    // The host comes back; a worker claimed `step` meanwhile.
    let step = rust(
        &rows.chunks(),
        "step",
        RustStatus::Claimed,
        Validation::Unvalidated,
    );
    rows.0.lock().unwrap().1.push(step);
    connection(feed, &rows, |marks| {
        marks.send(Mark::Applied).unwrap();
        match next(&rx) {
            Some(Msg::Rows {
                structure, snap, ..
            }) => {
                assert_eq!(structure, 1, "same rows: navigation and layout stay");
                assert!(
                    snap.status.iter().any(|s| s.claim.is_some()),
                    "the claim made while away arrived"
                );
            }
            _ => panic!("expected the program's rows"),
        }
        marks.send(Mark::Lost("done".into())).unwrap();
    });
}
