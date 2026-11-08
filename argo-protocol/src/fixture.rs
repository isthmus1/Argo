//! The ARCHITECTURE.md §5.6 worked fixture as protocol records: what W should publish for
//! `fixtures/compute/compute.c` once interfaces are derived, and a transcript of live updates
//! for GUI and server work before the real pipeline exists.
//!
//! Hand-written, not produced by a pipeline run. The transcript's reports, models and case counts
//! are illustrative records, not measured results.

use crate::{
    Analysis, AttemptId, Candidate, Claim, ClaimPhase, ContentHash, Direction, DispatchKind,
    Effect, Failure, FileId, Graph, GraphBuilder, GraphRevision, ImplementationId, Job, JobId,
    JobState, Language, LoopKind, NodeId, NodeKind, NodeStatus, PinSpec, ProjectId, Provenance,
    Relation, ReportId, RevisionId, RevisionState, Snapshot, SourceFile, SourceRange, Stage,
    SyntaxPart, SyntaxRole, TypeKind, Update, ValueType, Verdict, Worker, WorkerId,
    pin::key,
    work::{
        Abi, Annotation, AnnotationKind, Attempt, Author, Counterexample, Implementation, Outcome,
        Tier, ValidationReport,
    },
};

pub const SOURCE: &str = include_str!("../../fixtures/compute/compute.c");
pub const PATH: &str = "fixtures/compute/compute.c";
const FILE: FileId = FileId(1);

pub fn source() -> SourceFile {
    SourceFile::new(FILE, PATH, SOURCE)
}

/// Byte range of the `nth` occurrence of `needle`.
fn find(needle: &str, nth: usize) -> SourceRange {
    let (lo, _) = SOURCE
        .match_indices(needle)
        .nth(nth)
        .unwrap_or_else(|| panic!("{needle:?} in fixture"));
    SourceRange {
        file: FILE,
        lo: lo as u32,
        hi: (lo + needle.len()) as u32,
    }
}

fn at(needle: &str) -> Provenance {
    Provenance::at(find(needle, 0))
}

/// Node IDs worth naming in tests and transcripts.
#[derive(Debug, Clone, Copy)]
pub struct Ids {
    pub program: NodeId,
    pub module: NodeId,
    pub uint32: NodeId,
    pub normalize: NodeId,
    pub step: NodeId,
    pub compute: NodeId,
    pub step_body: NodeId,
    pub loop_region: NodeId,
    pub loop_dispatch: NodeId,
    pub loop_body: NodeId,
    pub call_step: NodeId,
    pub call_normalize: NodeId,
}

pub fn graph() -> (Graph, Ids) {
    let u32t = ValueType::u32;
    let mut b = GraphBuilder::new(GraphRevision {
        id: RevisionId(1),
        project: ProjectId(1),
        parent: None,
        analysis: Analysis::Interfaces,
        state: RevisionState::Draft,
    });
    b.file(source().file_ref());

    let program = b.node(None, NodeKind::Program, "fixture");
    let module = b.node(Some(program), NodeKind::Module, "compute.c");
    b.provenance(
        module,
        Provenance::at(SourceRange {
            file: FILE,
            lo: 0,
            hi: SOURCE.len() as u32,
        }),
    );

    let uint32 = b.node(Some(module), NodeKind::Type(TypeKind::Alias), "uint32_t");
    b.provenance(uint32, at("typedef unsigned int uint32_t;"));

    // Boundary interface shared by the function, its interior and its call sites.
    let function = |b: &mut GraphBuilder, name: &str, def: &str, params: &[&str]| {
        let f = b.node(Some(module), NodeKind::Function, name);
        let mut prov = at(def);
        prov.parts.push(SyntaxPart {
            role: SyntaxRole::Signature,
            range: find(def.lines().next().unwrap(), 0),
        });
        b.provenance(f, prov).effects(f, vec![Effect::Pure]);
        b.pins(
            f,
            [
                PinSpec::enter(),
                PinSpec::then().origin(crate::PinOrigin::Signature),
            ],
        );
        for p in params {
            b.pin(f, PinSpec::param(p, u32t()));
        }
        b.pin(f, PinSpec::ret(u32t()));
        b.relation(Relation::TypeUse {
            user: f,
            ty: uint32,
        });
        f
    };
    let region = |b: &mut GraphBuilder, parent: NodeId, kind: NodeKind, name: &str, text: &str| {
        let r = b.node(Some(parent), kind, name);
        b.provenance(r, at(text))
            .work_unit(r, true)
            .effects(r, vec![Effect::Pure]);
        b.pin(r, PinSpec::enter());
        r
    };
    let return_pins = |b: &mut GraphBuilder, r: NodeId| {
        b.pins(
            r,
            [
                PinSpec::exit(crate::Flow::Return),
                PinSpec::ret(u32t()).origin(crate::PinOrigin::Liveness),
            ],
        );
    };

    // normalize(x) = x & 255
    let normalize = function(
        &mut b,
        "normalize",
        "uint32_t normalize(uint32_t x)\n{\n    return x & 255;\n}",
        &["x"],
    );
    let gi = b.group_input(normalize);
    let seq = region(
        &mut b,
        normalize,
        NodeKind::Sequence,
        "return x & 255",
        "return x & 255;",
    );
    b.pin(seq, PinSpec::live("x", Direction::In, u32t()));
    return_pins(&mut b, seq);
    let go = b.group_output(normalize, key::THEN, &[key::RETURN]);
    b.exec((gi, key::ENTER), (seq, key::ENTER));
    b.data((gi, &key::param("x")), (seq, &key::live("x")));
    b.exec((seq, key::RETURN_EXIT), (go, key::THEN));
    b.data((seq, key::RETURN), (go, key::RETURN));

    // step(acc, i) = normalize(acc + i), with the sequence opened to statement detail.
    let step = function(
        &mut b,
        "step",
        "uint32_t step(uint32_t acc, uint32_t i)\n{\n    return normalize(acc + i);\n}",
        &["acc", "i"],
    );
    let gi = b.group_input(step);
    let step_body = region(
        &mut b,
        step,
        NodeKind::Sequence,
        "return normalize(acc + i)",
        "return normalize(acc + i);",
    );
    b.pins(
        step_body,
        [
            PinSpec::live("acc", Direction::In, u32t()),
            PinSpec::live("i", Direction::In, u32t()),
        ],
    );
    return_pins(&mut b, step_body);
    let go = b.group_output(step, key::THEN, &[key::RETURN]);
    b.exec((gi, key::ENTER), (step_body, key::ENTER));
    b.data((gi, &key::param("acc")), (step_body, &key::live("acc")));
    b.data((gi, &key::param("i")), (step_body, &key::live("i")));
    b.exec((step_body, key::RETURN_EXIT), (go, key::THEN));
    b.data((step_body, key::RETURN), (go, key::RETURN));
    let call_normalize = {
        let gi = b.group_input(step_body);
        let sum = b.node(Some(step_body), NodeKind::Expression, "acc + i");
        b.provenance(sum, at("acc + i"))
            .effects(sum, vec![Effect::Pure]);
        b.pins(
            sum,
            [
                PinSpec::operand(0, "acc", u32t()),
                PinSpec::operand(1, "i", u32t()),
                PinSpec::value_out(u32t()),
            ],
        );
        let call = b.call(step_body, normalize, true);
        b.provenance(call, at("normalize(acc + i)"));
        b.node_mut(call).name = "normalize(acc + i)".into();
        let go = b.group_output(step_body, key::RETURN_EXIT, &[key::RETURN]);
        b.data((gi, &key::live("acc")), (sum, &key::operand(0)));
        b.data((gi, &key::live("i")), (sum, &key::operand(1)));
        b.data((sum, key::VALUE), (call, &key::param("x")));
        b.data((call, key::RETURN), (go, key::RETURN));
        b.exec((gi, key::ENTER), (go, key::RETURN_EXIT));
        call
    };

    // compute(n): acc = 0; for (i = 0; i < n; i++) acc = step(acc, i); return acc;
    let compute = function(&mut b, "compute", "uint32_t compute(uint32_t n)\n{", &["n"]);
    // The whole definition, through the closing brace at the end of the file.
    b.node_mut(compute).provenance.as_mut().unwrap().range.hi = SOURCE.trim_end().len() as u32;
    let gi = b.group_input(compute);
    let init = region(
        &mut b,
        compute,
        NodeKind::Sequence,
        "uint32_t acc = 0",
        "uint32_t acc = 0;",
    );
    b.pins(
        init,
        [
            PinSpec::then(),
            PinSpec::live("acc", Direction::Out, u32t()),
        ],
    );
    let loop_region = region(
        &mut b,
        compute,
        NodeKind::Loop(LoopKind::For),
        "for (uint32_t i = 0; i < n; i++)",
        "for (uint32_t i = 0; i < n; i++)\n        acc = step(acc, i);",
    );
    b.pins(
        loop_region,
        [
            PinSpec::then(),
            PinSpec::live("n", Direction::In, u32t()),
            PinSpec::live("acc", Direction::InOut, u32t()),
        ],
    );
    let ret = region(
        &mut b,
        compute,
        NodeKind::Sequence,
        "return acc",
        "return acc;",
    );
    b.pin(ret, PinSpec::live("acc", Direction::In, u32t()));
    return_pins(&mut b, ret);
    let go = b.group_output(compute, key::THEN, &[key::RETURN]);
    b.exec((gi, key::ENTER), (init, key::ENTER));
    b.exec((init, key::THEN), (loop_region, key::ENTER));
    b.exec((loop_region, key::THEN), (ret, key::ENTER));
    b.exec((ret, key::RETURN_EXIT), (go, key::THEN));
    b.data((gi, &key::param("n")), (loop_region, &key::live("n")));
    b.data((init, &key::live("acc")), (loop_region, &key::live("acc")));
    b.data((loop_region, &key::live("acc")), (ret, &key::live("acc")));
    b.data((ret, key::RETURN), (go, key::RETURN));

    // Loop interior: the dispatch owns init/condition/step; acc is carried explicitly.
    let gi = b.group_input(loop_region);
    let loop_dispatch = b.node(
        Some(loop_region),
        NodeKind::Dispatch(DispatchKind::Loop),
        "for (uint32_t i = 0; i < n; i++)",
    );
    let mut prov = at("for (uint32_t i = 0; i < n; i++)");
    prov.parts = vec![
        SyntaxPart {
            role: SyntaxRole::Initializer,
            range: find("uint32_t i = 0", 0),
        },
        SyntaxPart {
            role: SyntaxRole::Condition,
            range: find("i < n", 0),
        },
        SyntaxPart {
            role: SyntaxRole::Step,
            range: find("i++", 0),
        },
    ];
    b.provenance(loop_dispatch, prov)
        .effects(loop_dispatch, vec![Effect::Pure]);
    b.pins(
        loop_dispatch,
        [
            PinSpec::enter(),
            PinSpec::backedge(),
            PinSpec::body(),
            PinSpec::completed(),
            PinSpec::live("n", Direction::In, u32t()),
            PinSpec::init("acc", u32t()),
            PinSpec::next("acc", u32t()),
            PinSpec::live("acc", Direction::Out, u32t()),
            PinSpec::live("i", Direction::Out, u32t()),
        ],
    );
    let loop_body = region(
        &mut b,
        loop_region,
        NodeKind::Sequence,
        "acc = step(acc, i)",
        "acc = step(acc, i);",
    );
    b.pins(
        loop_body,
        [
            PinSpec::then(),
            PinSpec::live("acc", Direction::InOut, u32t()),
            PinSpec::live("i", Direction::In, u32t()),
        ],
    );
    let go = b.group_output(loop_region, key::THEN, &["live.acc"]);
    b.exec((gi, key::ENTER), (loop_dispatch, key::ENTER));
    b.exec((loop_dispatch, key::BODY), (loop_body, key::ENTER));
    b.exec((loop_body, key::THEN), (loop_dispatch, key::NEXT));
    b.exec((loop_dispatch, key::COMPLETED), (go, key::THEN));
    b.data((gi, &key::live("n")), (loop_dispatch, &key::live("n")));
    b.data((gi, &key::live("acc")), (loop_dispatch, &key::init("acc")));
    b.data(
        (loop_dispatch, &key::live("acc")),
        (loop_body, &key::live("acc")),
    );
    b.data(
        (loop_dispatch, &key::live("i")),
        (loop_body, &key::live("i")),
    );
    b.data(
        (loop_body, &key::live("acc")),
        (loop_dispatch, &key::next("acc")),
    );
    b.data((loop_dispatch, &key::live("acc")), (go, &key::live("acc")));

    // Loop body interior: one call on the exec chain.
    let gi = b.group_input(loop_body);
    let call_step = b.call(loop_body, step, false);
    b.provenance(call_step, at("step(acc, i)"));
    b.node_mut(call_step).name = "step(acc, i)".into();
    let go = b.group_output(loop_body, key::THEN, &["live.acc"]);
    b.exec((gi, key::ENTER), (call_step, key::ENTER));
    b.exec((call_step, key::THEN), (go, key::THEN));
    b.data((gi, &key::live("acc")), (call_step, &key::param("acc")));
    b.data((gi, &key::live("i")), (call_step, &key::param("i")));
    b.data((call_step, key::RETURN), (go, &key::live("acc")));

    let ids = Ids {
        program,
        module,
        uint32,
        normalize,
        step,
        compute,
        step_body,
        loop_region,
        loop_dispatch,
        loop_body,
        call_step,
        call_normalize,
    };
    (b.finish(), ids)
}

/// Initial subscriber state: the frozen fixture graph, all original C, two idle workers.
pub fn snapshot() -> Snapshot {
    let (mut graph, _) = graph();
    graph
        .freeze()
        .expect("the fixture graph is structurally valid");
    let mut snap = Snapshot::new(graph, vec![source()]);
    for id in [1, 2] {
        snap.apply(Update::Worker(Worker {
            id: WorkerId(id),
            label: format!("worker {id}"),
            model: "fixture".into(),
            online: true,
        }));
    }
    snap
}

const MANIFEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn report(
    id: u64,
    implementation: u64,
    outcome: Outcome,
    passed: u32,
    counterexample: Option<Counterexample>,
) -> ValidationReport {
    ValidationReport {
        id: ReportId(id),
        implementation: ImplementationId(implementation),
        manifest_hash: ContentHash::new(MANIFEST),
        tier: Tier::Quick,
        outcome,
        cases_passed: passed,
        cases_total: 33,
        counterexample,
        log_excerpt: String::new(),
    }
}

fn rust(id: u64, node: NodeId, preview: &str) -> Implementation {
    Implementation {
        id: ImplementationId(id),
        node,
        language: Language::Rust,
        abi: Abi::C,
        source_hash: crate::hash::sha256(preview.as_bytes()),
        preview: Some(preview.into()),
    }
}

/// Live updates after [`snapshot`]: explanations arrive, `normalize` is ported, a deliberately
/// wrong `step` candidate is rejected with a counterexample, then a corrected one is accepted.
/// The result is the C → Rust → C manifest of §5.6 (C `compute`, Rust `step`, Rust `normalize`).
pub fn transcript() -> Vec<Update> {
    let (_, ids) = graph();
    let claim = |worker, job, attempt, phase| Claim {
        worker: WorkerId(worker),
        job: JobId(job),
        attempt: AttemptId(attempt),
        stage: Stage::Translate,
        phase,
    };
    let status = |node, accepted, f: &dyn Fn(&mut NodeStatus)| {
        let mut s = NodeStatus::new(node, accepted);
        f(&mut s);
        Update::Status(s)
    };
    let job = |id, target: NodeId, state| {
        Update::Job(Job {
            id: JobId(id),
            stage: Stage::Translate,
            logical_key: format!("translate:{}", target.0),
            targets: vec![target],
            prerequisites: Vec::new(),
            state,
        })
    };
    let normalize_rs =
        "#[unsafe(no_mangle)]\npub extern \"C\" fn normalize(x: u32) -> u32 {\n    x & 255\n}\n";
    let step_bad = "#[unsafe(no_mangle)]\npub extern \"C\" fn step(acc: u32, i: u32) -> u32 {\n    unsafe { normalize(acc) + i }\n}\n";
    let step_rs = "#[unsafe(no_mangle)]\npub extern \"C\" fn step(acc: u32, i: u32) -> u32 {\n    unsafe { normalize(acc.wrapping_add(i)) }\n}\n";
    let mismatch = Failure::Mismatch;
    vec![
        Update::Annotation(Annotation {
            node: ids.normalize,
            kind: AnnotationKind::Summary,
            text: "Keeps the low 8 bits of x.".into(),
            author: Author::Model {
                model: "fixture".into(),
                job: None,
            },
        }),
        job(1, ids.normalize, JobState::Running),
        Update::Attempt(Attempt {
            id: AttemptId(1),
            job: JobId(1),
            worker: WorkerId(1),
            fence: 1,
            lease_expires_ms: 0,
            state: JobState::Running,
        }),
        status(ids.normalize, Language::C, &|s| {
            s.claim = Some(claim(1, 1, 1, ClaimPhase::Running))
        }),
        job(2, ids.step, JobState::Queued),
        status(ids.step, Language::C, &|s| {
            s.claim = Some(claim(1, 2, 2, ClaimPhase::Queued))
        }),
        Update::Implementation(rust(1, ids.normalize, normalize_rs)),
        Update::Report(report(1, 1, Outcome::Passed, 33, None)),
        job(1, ids.normalize, JobState::Succeeded),
        status(ids.normalize, Language::Rust, &|s| {
            s.implementation = Some(ImplementationId(1));
            s.evidence = vec![ReportId(1)];
        }),
        job(2, ids.step, JobState::Running),
        status(ids.step, Language::C, &|s| {
            s.claim = Some(claim(1, 2, 2, ClaimPhase::Running))
        }),
        Update::Implementation(rust(2, ids.step, step_bad)),
        Update::Report(report(
            2,
            2,
            Outcome::Failed { failure: mismatch },
            9,
            Some(Counterexample {
                input: "acc = 250, i = 10".into(),
                expected: "4".into(),
                actual: "260".into(),
            }),
        )),
        status(ids.step, Language::C, &|s| {
            s.claim = Some(claim(1, 2, 3, ClaimPhase::Running));
            s.candidate = Some(Candidate {
                implementation: ImplementationId(2),
                language: Language::Rust,
                verdict: Verdict::Failed {
                    report: ReportId(2),
                    failure: mismatch,
                },
            });
        }),
        Update::Implementation(rust(3, ids.step, step_rs)),
        Update::Report(report(3, 3, Outcome::Passed, 33, None)),
        job(2, ids.step, JobState::Succeeded),
        status(ids.step, Language::Rust, &|s| {
            s.implementation = Some(ImplementationId(3));
            s.evidence = vec![ReportId(3)];
        }),
        job(3, ids.loop_body, JobState::Running),
        status(ids.loop_body, Language::C, &|s| {
            s.claim = Some(claim(2, 3, 4, ClaimPhase::Running))
        }),
    ]
}
