//! Snapshot → rows → snapshot must be lossless: the GUI rebuilds everything from rows alone.

use argo_db_client::{
    ChunkKind, Error, MAX_BATCH_BYTES, MAX_BATCH_ROWS, Origin, Places, ROOT, SEGMENT_BYTES, Unit,
    batches, origin as origin_of, rebuild, row_bytes, to_rows,
};
use argo_protocol::{
    FieldLayout, FileId, NodeKind, Snapshot, SourceFile, TypeLayout, ValueType, fixture,
};

fn origin() -> Origin {
    Origin {
        locator: "https://example.com/compute.git".into(),
        commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
        units: vec![Unit {
            path: "compute.c".into(),
            skipped: None,
        }],
        flags: vec!["-DN=32".into()],
    }
}

/// The fixture's source is the original file itself, so its own lines are the original ones.
fn own_lines(snap: &Snapshot) -> impl Fn(FileId, u32) -> Option<(String, u32)> + '_ {
    move |file, byte| {
        let f = snap.sources.iter().find(|f| f.id == file)?;
        let line = f.text.get(..byte as usize)?.matches('\n').count() as u32 + 1;
        Some((f.path.clone(), line))
    }
}

/// Workers are live state from `rust_chunk`, not ingested structure.
fn roundtrip(snap: &Snapshot) {
    let rows = to_rows(snap, &origin(), &own_lines(snap)).unwrap();
    let rebuilt = rebuild(rows.iter().rev()).unwrap();
    let expected = Snapshot {
        workers: vec![],
        ..snap.clone()
    };
    assert_eq!(rebuilt.snapshot.graph, expected.graph);
    assert_eq!(rebuilt.snapshot, expected);
    assert_eq!(rebuilt.origin, origin());
    assert_eq!(rebuilt.live([]), rebuilt.snapshot);
    assert_eq!(
        to_rows(snap, &origin(), &own_lines(snap)).unwrap(),
        rows,
        "ids and rows are deterministic"
    );
}

#[test]
fn fixture_snapshot_roundtrips() {
    roundtrip(&fixture::snapshot());
}

#[test]
fn golden_snapshot_json_roundtrips() {
    let json = include_str!("../../fixtures/compute/snapshot.json");
    roundtrip(&serde_json::from_str(json).unwrap());
}

#[test]
fn type_layouts_are_owned_by_their_type_chunk() {
    let mut snap = fixture::snapshot();
    let mut ty = snap.graph.nodes.iter();
    let ty = ty.find(|n| matches!(n.kind, NodeKind::Type(_))).unwrap().id;
    let field = |name: &str, offset| FieldLayout {
        name: name.into(),
        offset,
        ty: ValueType::Int {
            bits: 32,
            signed: false,
        },
    };
    let layout = |fields| TypeLayout {
        node: ty,
        size: 8,
        align: 4,
        fields,
    };
    snap.graph.layouts = vec![layout(vec![field("lo", 0)]), layout(vec![field("hi", 4)])];
    roundtrip(&snap);
    let rows = to_rows(&snap, &origin(), &own_lines(&snap)).unwrap();
    let owner = rows.iter().find(|r| r.node == Some(ty.0)).unwrap();
    assert_eq!(owner.kind, ChunkKind::Type);
    let records: serde_json::Value = serde_json::from_str(&owner.records).unwrap();
    assert_eq!(records["layouts"].as_array().unwrap().len(), 2);
}

#[test]
fn large_multibyte_sources_span_segments() {
    let mut snap = fixture::snapshot();
    let f = &snap.sources[0];
    let text = format!("{}/* {} */\n", f.text, "é".repeat(SEGMENT_BYTES));
    snap.sources[0] = SourceFile::new(f.id, f.path.clone(), text);
    let rows = to_rows(&snap, &origin(), &own_lines(&snap)).unwrap();
    assert_eq!(
        rows.iter().filter(|r| r.kind == ChunkKind::Source).count(),
        3
    );
    roundtrip(&snap);
}

#[test]
fn work_units_carry_their_exact_c_text() {
    let snap = fixture::snapshot();
    let rows = to_rows(&snap, &origin(), &own_lines(&snap)).unwrap();
    let text = &snap.sources[0];
    let units: Vec<_> = rows.iter().filter(|r| r.translatable).collect();
    let functions: Vec<_> = units
        .iter()
        .filter(|r| r.kind == ChunkKind::Function)
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(functions, ["normalize", "step", "compute"]);
    for r in &units {
        let node = snap
            .graph
            .nodes
            .iter()
            .find(|n| Some(n.id.0) == r.node)
            .unwrap();
        let range = node.provenance.as_ref().unwrap().range;
        assert_eq!(Some(r.source.as_str()), text.slice(range));
        // The fixture's source is the original file, recorded under its own path (the module
        // is named `compute.c`), so both locations name it.
        assert_eq!(r.path, text.path);
        assert_eq!(r.ingested, text.path);
        assert!(r.line_start >= 1 && r.line_start <= r.line_end);
    }
    let step = units.iter().find(|r| r.name == "step").unwrap();
    assert!(step.source.starts_with("uint32_t step(") && step.line_start == 9);
    // Nested work units are claimed before their function.
    for r in units.iter().filter(|r| r.kind == ChunkKind::Region) {
        let parent = rows.iter().find(|p| Some(p.id) == r.parent).unwrap();
        assert!(
            parent.translatable && parent.descendants > r.descendants && parent.depth < r.depth
        );
    }
}

#[test]
fn partial_ingest_is_reported_as_incomplete() {
    let rows = to_rows(&fixture::snapshot(), &origin(), &|_, _| None).unwrap();
    let want = rows.len() as u64;
    assert_eq!(
        rebuild(&rows[..rows.len() - 1]),
        Err(Error::Incomplete {
            have: want - 1,
            want
        })
    );
    assert_eq!(rebuild(&rows[1..]), Err(Error::NoProject));
    let mut dup = rows.clone();
    dup.pop();
    dup.push(rows[1].clone());
    assert!(matches!(rebuild(&dup), Err(Error::Corrupt(_))));
}

/// The worker's per-claim check reads the project row alone, with the same completeness rule.
#[test]
fn the_origin_is_read_from_the_project_row_once_complete() {
    let rows = to_rows(&fixture::snapshot(), &origin(), &|_, _| None).unwrap();
    let (root, want) = (rows.iter().find(|c| c.id == ROOT), rows.len() as u64);
    assert_eq!(root.map(|c| c.kind), Some(ChunkKind::Project));
    assert_eq!(origin_of(root, want), Ok(origin()));
    let have = want - 1;
    assert_eq!(origin_of(root, have), Err(Error::Incomplete { have, want }));
    assert_eq!(origin_of(None, want), Err(Error::NoProject));
    assert!(matches!(
        origin_of(Some(&rows[1]), want),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn batches_are_bounded_and_start_with_the_project() {
    let rows = to_rows(&fixture::snapshot(), &origin(), &|_, _| None).unwrap();
    let many: Vec<_> = (0..300).map(|_| rows[2].clone()).collect();
    let b = batches(many).unwrap();
    assert_eq!(
        b.iter().map(Vec::len).collect::<Vec<_>>(),
        [MAX_BATCH_ROWS, MAX_BATCH_ROWS, 44]
    );
    assert_eq!(batches(rows).unwrap()[0][0].kind, ChunkKind::Project);
}

/// Name and path count toward the byte bound, and a row too big for any call is an error.
#[test]
fn batch_bytes_count_names_and_paths() {
    let rows = to_rows(&fixture::snapshot(), &origin(), &|_, _| None).unwrap();
    let mut big = rows[2].clone();
    big.name = "n".repeat(MAX_BATCH_BYTES / 2);
    big.path = "p".repeat(MAX_BATCH_BYTES / 4);
    assert!(row_bytes(&big) > big.name.len() + big.path.len());
    let b = batches(vec![big.clone(), big.clone()]).unwrap();
    assert_eq!(
        b.len(),
        2,
        "two rows over the bound together go in separate calls"
    );
    big.name.push_str(&"n".repeat(MAX_BATCH_BYTES / 2));
    let id = big.id;
    assert!(matches!(batches(vec![big]), Err(Error::RowTooLarge { id: i, .. }) if i == id));
}

/// A chunk whose ends can't be traced to one original file keeps its module path and claims no
/// lines, rather than presenting ingested-text positions as repository ones.
#[test]
fn untraceable_chunks_report_no_lines() {
    let snap = fixture::snapshot();
    let none = |_: FileId, _: u32| -> Option<(String, u32)> { None };
    let split = |_: FileId, byte: u32| Some((format!("f{}.c", byte % 2), 1));
    for places in [&none as &Places, &split] {
        let rows = to_rows(&snap, &origin(), places).unwrap();
        let step = rows.iter().find(|r| r.name == "step").unwrap();
        assert_eq!(
            (step.path.as_str(), step.line_start, step.line_end),
            ("compute.c", 0, 0)
        );
        assert!(step.source.starts_with("uint32_t step(") && step.ingested == snap.sources[0].path);
    }
}
