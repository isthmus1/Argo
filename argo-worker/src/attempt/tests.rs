//! The retry loop against a scripted model and validator.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

use argo_db_client::{CChunk, ChunkKind, Validation};

use super::{Budget, Escalation, OUTAGES, Outcome, translate};
use crate::model::{Model, ModelError};
use crate::validate::{Candidate, Validator, Verdict};

const GOOD: &str =
    "```rust\n#[no_mangle]\npub extern \"C\" fn add(a: i32, b: i32) -> i32 { a + b }\n```";

type Reply = Result<String, ModelError>;

fn ok(text: &str) -> Reply {
    Ok(text.into())
}

fn bad(why: &str) -> Reply {
    Err(ModelError::Reply(why.into()))
}

fn down(why: &str) -> Reply {
    Err(ModelError::Unavailable(why.into()))
}

/// Replies in order; records which model each prompt went to.
struct Script {
    replies: RefCell<VecDeque<Reply>>,
    calls: RefCell<Vec<(String, String)>>,
}

fn script(replies: impl IntoIterator<Item = Reply>) -> Script {
    Script {
        replies: RefCell::new(replies.into_iter().collect()),
        calls: RefCell::default(),
    }
}

impl Model for Script {
    async fn complete(&self, model: &str, prompt: &str) -> Reply {
        self.calls.borrow_mut().push((model.into(), prompt.into()));
        self.replies
            .borrow_mut()
            .pop_front()
            .expect("more calls than scripted")
    }
}

struct Verdicts(RefCell<VecDeque<Verdict>>);

impl Validator for Verdicts {
    async fn validate(&self, c: &Candidate<'_>) -> Verdict {
        assert!(!c.source.contains("```"), "fences reach the validator");
        self.0
            .borrow_mut()
            .pop_front()
            .expect("more validations than scripted")
    }
}

fn verdicts(v: impl IntoIterator<Item = Verdict>) -> Verdicts {
    Verdicts(RefCell::new(v.into_iter().collect()))
}

fn chunk() -> CChunk {
    CChunk {
        id: 7,
        node: Some(3),
        parent: Some(1),
        name: "add".into(),
        kind: ChunkKind::Function,
        translatable: true,
        depth: 2,
        descendants: 0,
        path: "add.c".into(),
        line_start: 1,
        line_end: 1,
        ingested: "all.c".into(),
        byte_lo: 0,
        byte_hi: 40,
        source: "int add(int a, int b) { return a + b; }".into(),
        records: "{}".into(),
    }
}

fn budget(max_attempts: u32, escalation: Option<(&str, u32)>) -> Budget {
    Budget {
        max_attempts,
        model: "base".into(),
        escalation: escalation.map(|(m, after)| Escalation {
            model: m.into(),
            after,
        }),
        backoff: Duration::ZERO,
    }
}

/// The prompt names the previous failure so the test can see it was fed back.
fn prompt(feedback: Option<&str>) -> String {
    format!(
        "translate; previous failure: {}",
        feedback.unwrap_or("none")
    )
}

async fn run(model: &Script, v: &Verdicts, b: &Budget) -> Outcome {
    translate(model, v, b, &chunk(), None, prompt).await
}

#[tokio::test]
async fn first_unvalidated_candidate_is_submitted_without_fences() {
    let (model, v) = (
        script([ok(GOOD)]),
        verdicts([Verdict::Unvalidated("unchecked".into())]),
    );
    let Outcome::Done {
        source,
        model: used,
        attempts,
        validation,
        log,
    } = run(&model, &v, &budget(3, None)).await
    else {
        panic!("expected Done");
    };
    assert!(source.starts_with("#[no_mangle]") && source.ends_with("a + b }"));
    assert_eq!(
        (used.as_str(), attempts, validation),
        ("base", 1, Validation::Unvalidated)
    );
    assert_eq!(log, "attempt 1 (base): unchecked\n");
}

#[tokio::test]
async fn failures_are_fed_back_and_escalate_after_k() {
    let model = script([bad("timeout"), ok("let x = 1;"), ok(GOOD), ok(GOOD)]);
    let v = verdicts([
        Verdict::Failed("link error: undefined add".into()),
        Verdict::Passed("ok".into()),
    ]);
    let outcome = run(&model, &v, &budget(4, Some(("strong", 2)))).await;
    let calls = model.calls.borrow();
    let models: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(models, ["base", "base", "strong", "strong"]);
    assert!(calls[0].1.ends_with("none"));
    assert!(calls[1].1.contains("timeout"), "model error fed back");
    assert!(
        calls[2].1.contains("#[no_mangle] pub extern \"C\""),
        "shape failure fed back"
    );
    assert!(
        calls[3].1.contains("link error: undefined add"),
        "validator log fed back"
    );
    let Outcome::Done {
        model,
        attempts,
        validation,
        log,
        ..
    } = outcome
    else {
        panic!("expected Done");
    };
    assert_eq!(
        (model.as_str(), attempts, validation),
        ("strong", 4, Validation::Passed)
    );
    assert_eq!(log.lines().filter(|l| l.starts_with("attempt ")).count(), 4);
}

#[tokio::test]
async fn persistent_failure_ends_failed_after_max_attempts() {
    let model = script([ok("I can't do that."), ok(GOOD), bad("HTTP 413")]);
    let v = verdicts([Verdict::Failed("tests failed".into())]);
    let outcome = run(&model, &v, &budget(3, None)).await;
    let Outcome::Failed { attempts, log } = outcome else {
        panic!("expected Failed");
    };
    assert_eq!(attempts, 3);
    assert!(log.contains("attempt 1 (base): no exported function"));
    assert!(log.contains("attempt 2 (base): validation failed:\ntests failed"));
    assert!(log.contains("attempt 3 (base): HTTP 413"));
    assert!(model.replies.borrow().is_empty() && v.0.borrow().is_empty());
}

#[tokio::test]
async fn a_renamed_function_is_rejected_before_validation() {
    let renamed = "#[no_mangle]\npub extern \"C\" fn add_two(a: i32) -> i32 { a }";
    let (model, v) = (script([ok(renamed)]), verdicts([]));
    let Outcome::Failed { log, .. } = run(&model, &v, &budget(1, None)).await else {
        panic!("expected Failed");
    };
    assert!(log.contains("no `fn add(`"), "{log}");
}

#[tokio::test]
async fn outages_use_no_attempt() {
    let model = script([down("connection refused"), down("HTTP 429"), ok(GOOD)]);
    let v = verdicts([Verdict::Unvalidated("unchecked".into())]);
    let Outcome::Done { attempts, log, .. } = run(&model, &v, &budget(1, None)).await else {
        panic!("expected Done");
    };
    assert_eq!(attempts, 1);
    assert!(log.contains("outage 2 (base): HTTP 429"), "{log}");
    let prompts = model.calls.borrow();
    assert!(
        prompts.iter().all(|(_, p)| p.ends_with("none")),
        "no feedback"
    );
}

#[tokio::test]
async fn a_lasting_outage_leaves_the_chunk_unfailed() {
    let replies = (0..=OUTAGES).map(|_| down("OPENAI_API_KEY is not set"));
    let model = script(replies);
    let outcome = run(&model, &verdicts([]), &budget(3, None)).await;
    let Outcome::Unavailable { log } = outcome else {
        panic!("expected Unavailable, not {outcome:?}");
    };
    assert!(!log.contains("attempt "), "{log}");
    assert!(model.replies.borrow().is_empty());
}
