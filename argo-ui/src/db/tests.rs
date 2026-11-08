//! Rows → canvas, without a database: the rows `argo-server` would ingest for the protocol
//! fixture, `rust_chunk` rows as workers would write them, delivered as the subscription would.

use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use argo_db_client::{
    CChunk, LEASE_MICROS, Origin, RustChunk, RustStatus, Validation, rebuild, to_rows,
};
use argo_protocol::fixture;
use spacetimedb_sdk::{Identity, Timestamp};

use super::{Link, Msg, first, poll};
use crate::app::{Item, State};
use crate::model::{Evidence, Lang};
use crate::project;

fn origin() -> Origin {
    Origin {
        locator: "https://example.com/compute.git".into(),
        commit: Some("0123abcd".into()),
        units: vec![],
        flags: vec![],
    }
}

pub(super) fn chunks() -> Vec<CChunk> {
    to_rows(&fixture::snapshot(), &origin(), &|_, _| None).unwrap()
}

/// A row as a worker writes it, just now (a fresh claim holds its lease).
pub(super) fn rust(
    rows: &[CChunk],
    name: &str,
    status: RustStatus,
    validation: Validation,
) -> RustChunk {
    let chunk = rows
        .iter()
        .find(|r| r.name == name && r.translatable)
        .unwrap()
        .id;
    RustChunk {
        chunk,
        status,
        worker: Identity::from_byte_array([7; 32]),
        generation: 1,
        claimed_at: Timestamp::from_micros_since_unix_epoch(1_000),
        updated_at: Timestamp::now(),
        attempts: 1,
        model: "nemotron".into(),
        source: if status == RustStatus::Claimed {
            String::new()
        } else {
            format!("#[unsafe(no_mangle)]\npub extern \"C\" fn {name}() {{}}")
        },
        validation,
        log: String::new(),
    }
}

/// What the rebuilder sends for these rows.
fn rows(structure: u64, rust: &[RustChunk]) -> Msg {
    let s = rebuild(&chunks()).unwrap();
    Msg::Rows {
        structure,
        snap: Box::new(s.live(rust)),
        origin: s.origin,
    }
}

/// A window showing `compute` with its loop opened (where `step` is called), as the subscription
/// first delivers it, and the feed's sender.
fn window(rust: &[RustChunk]) -> (State, Sender<Msg>) {
    let (tx, rx) = channel();
    tx.send(Msg::Online(true)).unwrap();
    tx.send(rows(1, rust)).unwrap();
    let mut s = first(Link::new("argo".into(), rx), Duration::from_secs(1)).unwrap();
    let (_, ids) = fixture::graph();
    project::open_function(&mut s, ids.compute);
    s.live
        .as_mut()
        .unwrap()
        .open
        .extend([ids.loop_region, ids.loop_body]);
    s.reproject();
    s.relayout();
    (s, tx)
}

/// The call site of `step` inside `compute`, which shows `step`'s status.
fn step(s: &State) -> &crate::model::Node {
    &s.view.nodes[s.find("step(acc, i)").expect("step call in compute")]
}

#[test]
fn rust_rows_become_node_visuals() {
    let c = chunks();
    let (mut s, tx) = window(&[rust(
        &c,
        "step",
        RustStatus::Claimed,
        Validation::Unvalidated,
    )]);
    assert!(s.online && s.db.is_some() && s.project.is_some());
    assert_eq!(s.view.crumbs, ["fixture", "compute.c", "compute"]);
    let n = step(&s);
    assert!(
        n.claim.is_some_and(|c| c.active),
        "a placeholder is a running claim"
    );
    assert_eq!((n.lang, n.evidence), (Lang::C, Evidence::None));

    let cases = [
        // A worker that finished the node put Rust in the database: the node reads RS, and the
        // badge says nothing has judged it yet.
        (
            RustStatus::Done,
            Validation::Unvalidated,
            Lang::Rust,
            Evidence::Unvalidated,
        ),
        (
            RustStatus::Done,
            Validation::Passed,
            Lang::Rust,
            Evidence::Accepted,
        ),
        (
            RustStatus::Failed,
            Validation::Unvalidated,
            Lang::C,
            Evidence::Rejected,
        ),
    ];
    for (status, validation, lang, evidence) in cases {
        tx.send(rows(1, &[rust(&c, "step", status, validation)]))
            .unwrap();
        assert!(poll(&mut s));
        let n = step(&s);
        assert_eq!(
            (n.lang, n.evidence, n.claim),
            (lang, evidence, None),
            "{status:?} {validation:?}"
        );
    }
}

#[test]
fn the_inspector_reads_rust_from_the_rows() {
    let c = chunks();
    let (s, _tx) = window(&[rust(&c, "step", RustStatus::Done, Validation::Unvalidated)]);
    let live = s.live.as_ref().unwrap();
    let i = s.find("step(acc, i)").unwrap();
    let source = crate::bridge::rust_source(&live.snap, live.origin[i]).unwrap();
    assert!(source.contains("extern \"C\" fn step"));
}

#[test]
fn updates_keep_the_shown_function_selection_and_camera() {
    let c = chunks();
    let (mut s, tx) = window(&[]);
    let i = s.find("step(acc, i)").unwrap();
    s.select(Item::Node(i));
    s.cam.pan = [123.0, -45.0];
    let (container, pos) = (s.live.as_ref().unwrap().container, s.placed.pos.clone());

    // A worker claims `step`: status only.
    tx.send(rows(
        1,
        &[rust(
            &c,
            "step",
            RustStatus::Claimed,
            Validation::Unvalidated,
        )],
    ))
    .unwrap();
    poll(&mut s);
    assert_eq!(
        (s.selected, s.cam.pan),
        (Some(Item::Node(i)), [123.0, -45.0])
    );
    assert_eq!(s.live.as_ref().unwrap().container, container);
    assert_eq!(s.placed.pos, pos, "a status change never moves nodes");
    assert!(step(&s).claim.is_some());

    // A re-ingest delivers new structure; the same function and node stay shown and selected.
    tx.send(rows(
        2,
        &[rust(&c, "step", RustStatus::Done, Validation::Passed)],
    ))
    .unwrap();
    poll(&mut s);
    assert_eq!(s.live.as_ref().unwrap().container, container);
    let Some(Item::Node(j)) = s.selected else {
        panic!("selection lost: {:?}", s.selected)
    };
    assert_eq!(s.view.nodes[j].title, "step(acc, i)");
    assert_eq!(s.cam.pan, [123.0, -45.0]);
    assert_eq!(step(&s).evidence, Evidence::Accepted);
}

#[test]
fn rows_that_stop_describing_a_program_clear_the_canvas() {
    let (mut s, tx) = window(&[]);
    tx.send(Msg::Waiting("ingest incomplete: 3/12 rows".into()))
        .unwrap();
    poll(&mut s);
    assert!(s.live.is_none() && s.project.is_none() && s.view.nodes.is_empty());
    assert_eq!(s.view.crumbs, ["argo", "ingest incomplete: 3/12 rows"]);
    assert!(s.online, "still connected");

    // The next complete ingest shows again.
    tx.send(rows(3, &[])).unwrap();
    poll(&mut s);
    assert!(s.live.is_some() && !s.view.nodes.is_empty());
}

#[test]
fn navigation_survives_a_spell_of_incomplete_rows() {
    let (mut s, tx) = window(&[]);
    let i = s.find("step(acc, i)").unwrap();
    s.select(Item::Node(i));
    s.cam.pan = [123.0, -45.0];
    let (container, open) = s
        .live
        .as_ref()
        .map(|l| (l.container, l.open.clone()))
        .unwrap();

    tx.send(Msg::Waiting("ingest incomplete: 3/12 rows".into()))
        .unwrap();
    poll(&mut s);
    assert!(
        s.live.is_none(),
        "no stale graph while the rows are incomplete"
    );
    tx.send(rows(2, &[])).unwrap();
    poll(&mut s);

    let live = s.live.as_ref().unwrap();
    assert_eq!(
        (live.container, &live.open),
        (container, &open),
        "same function, same constructs open"
    );
    let Some(Item::Node(j)) = s.selected else {
        panic!("selection lost: {:?}", s.selected)
    };
    assert_eq!(s.view.nodes[j].title, "step(acc, i)");
    assert_eq!(s.cam.pan, [123.0, -45.0]);
}

#[test]
fn a_claim_whose_lease_runs_out_goes_idle() {
    let c = chunks();
    let mut row = rust(&c, "step", RustStatus::Claimed, Validation::Unvalidated);
    // Last renewed two seconds short of a whole lease ago, and never again: the worker died.
    let renewed = Timestamp::now().to_micros_since_unix_epoch() - LEASE_MICROS + 2_000_000;
    row.updated_at = Timestamp::from_micros_since_unix_epoch(renewed);
    let (mut s, _tx) = window(&[row]);
    assert!(step(&s).claim.is_some_and(|c| c.active));
    assert!(!poll(&mut s), "nothing changed yet");

    std::thread::sleep(Duration::from_millis(2_200));
    assert!(poll(&mut s), "the lease ran out with no row changing");
    assert!(
        step(&s).claim.is_some_and(|c| !c.active),
        "still claimed, but nobody is translating"
    );
}

/// A program built from scratch, delivered as its rows: what `argo-server` ingests for it.
fn show_rows(build: impl FnOnce(&mut argo_protocol::GraphBuilder), units: &[(&str, Option<&str>)]) -> State {
    use argo_db_client::Unit;
    use argo_protocol::{Analysis, GraphBuilder, GraphRevision, ProjectId, RevisionId, RevisionState, Snapshot};
    let mut b = GraphBuilder::new(GraphRevision {
        id: RevisionId(1),
        project: ProjectId(1),
        parent: None,
        analysis: Analysis::Interfaces,
        state: RevisionState::Draft,
    });
    build(&mut b);
    let origin = Origin {
        locator: "/src/demo".into(),
        commit: None,
        units: units.iter().map(|(path, skipped)| Unit { path: (*path).into(), skipped: skipped.map(Into::into) }).collect(),
        flags: vec![],
    };
    let rows = to_rows(&Snapshot::new(b.finish(), vec![]), &origin, &|_, _| None).unwrap();
    let rebuilt = rebuild(&rows).unwrap();
    let (tx, rx) = channel();
    tx.send(Msg::Rows {
        structure: 1,
        snap: Box::new(rebuilt.live(&[])),
        origin: rebuilt.origin,
    })
    .unwrap();
    first(Link::new("argo".into(), rx), Duration::from_secs(1)).unwrap()
}

/// The explorer's index from the rows alone: files with functions sorted by path, `main` in a .c
/// file shown first with its file expanded, and the counts the explorer prints.
#[test]
fn the_explorer_indexes_the_rows_and_opens_main_first() {
    use argo_protocol::NodeKind;
    let (mut main, mut main_c) = (None, None);
    let s = show_rows(
        |b| {
            let program = b.node(None, NodeKind::Program, "demo");
            let header = b.node(Some(program), NodeKind::Module, "lib.h");
            b.node(Some(header), NodeKind::Function, "helper");
            let m = b.node(Some(program), NodeKind::Module, "main.c");
            b.node(Some(m), NodeKind::Function, "setup");
            main = Some(b.node(Some(m), NodeKind::Function, "main"));
            main_c = Some(m);
            let a_c = b.node(Some(program), NodeKind::Module, "a.c");
            b.node(Some(a_c), NodeKind::Function, "alpha");
            b.node(Some(program), NodeKind::Module, "empty.c");
        },
        &[("a.c", None), ("main.c", None), ("broken.c", Some("parse error"))],
    );
    let (main, main_c) = (main.unwrap(), main_c.unwrap());

    assert_eq!(project::current(&s), Some((main, "main".into())));
    assert_eq!(s.view.crumbs, ["demo", "main.c", "main"]);
    let p = s.project.as_ref().unwrap();
    let names: Vec<_> = p.modules.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["a.c", "lib.h", "main.c"], "sorted, files without functions left out");
    assert_eq!((p.included(), p.units.len(), p.function_count()), (2, 3, 4));
    assert_eq!(p.module_of(main).map(|m| m.id), Some(main_c));
    assert_eq!(s.forms.expanded, Some(main_c), "the shown function's file is expanded");
    assert!(p.checkout.contains("no commit recorded"), "{}", p.checkout);
}

/// Rows with declarations but no function bodies: the explorer says what there is, and the
/// canvas shows the one file's level with those declarations on it.
#[test]
fn a_program_without_functions_shows_its_files_declarations() {
    use argo_protocol::{NodeKind, TypeKind};
    let s = show_rows(
        |b| {
            let program = b.node(None, NodeKind::Program, "settings");
            let config = b.node(Some(program), NodeKind::Module, "config.c");
            b.node(Some(config), NodeKind::State, "verbose");
            b.node(Some(config), NodeKind::Type(TypeKind::Struct), "options");
        },
        &[("config.c", None)],
    );
    let p = s.project.as_ref().unwrap();
    assert!(p.modules.is_empty());
    assert_eq!(p.empty_note(), "2 declarations, no function bodies");
    assert_eq!(project::current(&s).map(|(_, n)| n).as_deref(), Some("config.c"));
    let mut titles: Vec<&str> = s.view.nodes.iter().map(|n| n.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["options", "verbose"]);
}

#[test]
fn no_rows_within_the_timeout_is_an_error_naming_why() {
    let (tx, rx) = channel();
    tx.send(Msg::Waiting("no project has been ingested".into()))
        .unwrap();
    let e = first(Link::new("argo".into(), rx), Duration::from_millis(50))
        .err()
        .unwrap();
    assert!(e.contains("no project has been ingested"), "{e}");
}

/// Against a running host with an unclaimed chunk left: the window's feed sees a claim made
/// elsewhere (the `spacetime` CLI), without reconnecting.
/// `ARGO_DB=ws://127.0.0.1:3104 cargo test -p argo-ui live_feed -- --ignored`
#[test]
#[ignore = "needs ARGO_DB pointing at a running argo-db"]
fn live_feed_follows_the_database() {
    let uri = std::env::var("ARGO_DB").expect("ARGO_DB=ws://host:port");
    let module = std::env::var("ARGO_MODULE").unwrap_or_else(|_| "argo".into());
    let mut s = first(super::connect(&uri, &module), Duration::from_secs(20)).unwrap();
    let claims = |s: &State| {
        s.live
            .as_ref()
            .unwrap()
            .snap
            .status
            .iter()
            .filter(|st| st.claim.is_some())
            .count()
    };
    let before = claims(&s);
    let http = uri.replacen("ws", "http", 1);
    let out = std::process::Command::new("spacetime")
        .args(["call", "-s", &http, &module, "claim_next"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while claims(&s) == before && std::time::Instant::now() < deadline {
        poll(&mut s);
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        claims(&s),
        before + 1,
        "the new claim arrived through the subscription"
    );
    assert!(s.online);
}
