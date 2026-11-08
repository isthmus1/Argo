//! The agent loop against a scripted model and validator.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

use argo_db_client::{CChunk, ChunkKind, Validation};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::{Budget, OUTAGES, Outcome, Task, compact, escape_controls, translate};
use crate::model::{Message, Model, ModelError, Reply, ToolCall, ToolSpec};
use crate::validate::{Candidate, Failure, Stage, Validator, Verdict};

const GOOD: &str = "#[no_mangle]\npub extern \"C\" fn add(a: i32, b: i32) -> i32 { a + b }";

type Turn = Result<Reply, ModelError>;

fn call(id: &str, name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args.to_string(),
    }
}

fn calls(calls: Vec<ToolCall>) -> Turn {
    Ok(Reply {
        content: None,
        calls,
    })
}

fn submit(id: &str, source: &str) -> Turn {
    calls(vec![call(
        id,
        "submit_rewrite",
        json!({ "rust_source": source }),
    )])
}

fn text(t: &str) -> Turn {
    Ok(Reply {
        content: Some(t.into()),
        calls: vec![],
    })
}

/// Turns in order; records which model each went to and the conversation it saw.
struct Script {
    turns: RefCell<VecDeque<Turn>>,
    seen: RefCell<Vec<(String, Vec<Message>)>>,
}

fn script(turns: impl IntoIterator<Item = Turn>) -> Script {
    Script {
        turns: RefCell::new(turns.into_iter().collect()),
        seen: RefCell::default(),
    }
}

impl Model for Script {
    async fn chat(&self, model: &str, messages: &[Message], tools: &[ToolSpec]) -> Turn {
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        assert_eq!(names, ["read", "grep", "glob", "submit_rewrite"]);
        self.seen
            .borrow_mut()
            .push((model.into(), messages.to_vec()));
        self.turns
            .borrow_mut()
            .pop_front()
            .expect("more turns than scripted")
    }
}

impl Script {
    /// The content of the last message the model saw on turn `n` (0-based).
    fn last_seen(&self, n: usize) -> String {
        match self.seen.borrow()[n].1.last() {
            Some(Message::Tool { content, .. } | Message::User(content)) => content.clone(),
            other => panic!("{other:?}"),
        }
    }
}

struct Verdicts(RefCell<VecDeque<Verdict>>);

impl Validator for Verdicts {
    async fn validate(&self, c: &Candidate<'_>) -> Verdict {
        assert!(!c.source.contains("```"), "fences reach the validator");
        assert_eq!(
            std::fs::read_to_string(c.file).unwrap(),
            c.source,
            "saved first"
        );
        assert!(c.dir.ends_with(c.file.parent().unwrap()));
        self.0
            .borrow_mut()
            .pop_front()
            .expect("more validations than scripted")
    }
}

fn verdicts(v: impl IntoIterator<Item = Verdict>) -> Verdicts {
    Verdicts(RefCell::new(v.into_iter().collect()))
}

fn link_failure() -> Verdict {
    Verdict::Failed(vec![Failure {
        stage: Stage::Link,
        summary: "linking failed".into(),
        notes: vec!["`helper` is undefined".into()],
        log: Some("ld: undefined reference to `helper'\n".into()),
    }])
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

fn budget(max_attempts: u32) -> Budget {
    Budget {
        max_attempts,
        max_turns: 20,
        model: "base".into(),
        backoff: Duration::ZERO,
    }
}

/// A clone holding `add.c`, and an empty session directory.
struct Dirs {
    repo: TempDir,
    session: TempDir,
}

fn dirs() -> Dirs {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(
        repo.path().join("add.c"),
        "int add(int a, int b) { return a + b; }\n",
    )
    .unwrap();
    Dirs {
        repo,
        session: tempfile::tempdir().unwrap(),
    }
}

async fn run(model: &Script, v: &Verdicts, b: &Budget, d: &Dirs) -> Outcome {
    let chunk = chunk();
    let task = Task {
        chunk: &chunk,
        checkout: Some(d.repo.path()),
        session: d.session.path(),
        program: None,
    };
    translate(model, v, b, &task).await
}

#[tokio::test]
async fn the_agent_reads_then_an_accepted_submission_ends_the_loop() {
    let d = dirs();
    let model = script([
        calls(vec![
            call("r1", "read", json!({"path": "add.c"})),
            call("g1", "grep", json!({"pattern": "return"})),
        ]),
        submit("s1", GOOD),
    ]);
    let v = verdicts([Verdict::Unvalidated("compiled; linked".into())]);
    let outcome = run(&model, &v, &budget(3), &d).await;
    let Outcome::Done {
        source,
        model: used,
        attempts,
        validation,
        log,
    } = outcome
    else {
        panic!("expected Done, got {outcome:?}");
    };
    assert_eq!(source, GOOD, "the raw source, untouched");
    assert_eq!((used.as_str(), attempts), ("base", 1));
    assert_eq!(validation, Validation::Unvalidated);
    assert_eq!(log, "submission 1 (base): accepted: compiled; linked\n");
    // Both tool results came back, in order, before the second turn.
    let seen = model.seen.borrow();
    let results: Vec<&String> = seen[1]
        .1
        .iter()
        .filter_map(|m| match m {
            Message::Tool { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert!(
        results[0].contains("1\tint add(int a, int b)"),
        "{}",
        results[0]
    );
    assert!(results[1].starts_with("add.c:1: int add"), "{}", results[1]);
    assert!(d.session.path().join("submission-1/lib.rs").exists());
}

#[tokio::test]
async fn a_rejection_is_answered_with_reasons_and_log_paths_and_the_agent_resubmits() {
    let d = dirs();
    let model = script([
        submit("s1", "#[no_mangle] pub extern \"C\" fn add() {}"),
        submit("s2", GOOD),
    ]);
    let v = verdicts([link_failure(), Verdict::Unvalidated("ok".into())]);
    let outcome = run(&model, &v, &budget(3), &d).await;
    assert!(
        matches!(outcome, Outcome::Done { attempts: 2, .. }),
        "{outcome:?}"
    );
    let answer = model.last_seen(1);
    let log = d.session.path().join("submission-1/link.log");
    for expected in [
        "REJECTED: submission 1",
        "[link] linking failed",
        "- `helper` is undefined",
        &format!("full log: {}", log.display()),
        "undefined reference to `helper'",
        "2 more rejection(s)",
    ] {
        assert!(
            answer.contains(expected),
            "missing {expected:?} in:\n{answer}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "ld: undefined reference to `helper'\n"
    );
    // The same conversation: the rejected code is still there for the agent to fix.
    let seen = model.seen.borrow();
    assert!(seen[1].1.iter().any(|m| matches!(m,
        Message::Assistant(r) if r.calls.iter().any(|c| c.arguments.contains("fn add() {}")))));
}

#[tokio::test]
async fn markdown_is_rejected_not_stripped() {
    let d = dirs();
    let fenced = format!("```rust\n{GOOD}\n```");
    let model = script([submit("s1", &fenced), submit("s2", GOOD)]);
    let v = verdicts([Verdict::Unvalidated("ok".into())]);
    let Outcome::Done {
        source,
        attempts,
        log,
        ..
    } = run(&model, &v, &budget(3), &d).await
    else {
        panic!("expected Done");
    };
    assert_eq!((source.as_str(), attempts), (GOOD, 2));
    assert!(
        model
            .last_seen(1)
            .contains("[shape] `rust_source` must be raw Rust")
    );
    assert!(
        log.starts_with("submission 1 (base): shape: `rust_source` must be raw Rust"),
        "{log}"
    );
}

#[tokio::test]
async fn running_out_of_rejections_fails_the_chunk() {
    let d = dirs();
    let model = script([submit("s1", GOOD), submit("s2", GOOD)]);
    let v = verdicts([link_failure(), link_failure()]);
    let Outcome::Failed { attempts, log } = run(&model, &v, &budget(2), &d).await else {
        panic!("expected Failed");
    };
    assert_eq!(attempts, 2);
    assert_eq!(
        log.lines()
            .filter(|l| l.contains(": link: linking failed"))
            .count(),
        2
    );
    assert!(model.turns.borrow().is_empty());
}

#[tokio::test]
async fn text_replies_are_nudged_and_turns_are_bounded() {
    let d = dirs();
    let model = script([
        text("I will now write it."),
        text("Here it is"),
        text("done"),
    ]);
    let b = Budget {
        max_turns: 3,
        ..budget(3)
    };
    let Outcome::Failed { attempts, log } = run(&model, &verdicts([]), &b, &d).await else {
        panic!("expected Failed");
    };
    assert_eq!(attempts, 0);
    assert!(model.last_seen(1).starts_with("You must call a tool"));
    assert!(
        log.ends_with("gave up after 3 turns without an accepted submission\n"),
        "{log}"
    );
}

#[tokio::test]
async fn malformed_calls_and_unknown_tools_are_answered_not_submitted() {
    let d = dirs();
    let model = script([
        calls(vec![
            call("a", "submit_rewrite", json!({"code": GOOD})),
            call("b", "bash", json!({"command": "rm -rf /"})),
            ToolCall {
                id: "c".into(),
                name: "read".into(),
                arguments: "not json".into(),
            },
        ]),
        submit("s1", GOOD),
    ]);
    let v = verdicts([Verdict::Unvalidated("ok".into())]);
    let outcome = run(&model, &v, &budget(3), &d).await;
    assert!(
        matches!(outcome, Outcome::Done { attempts: 1, .. }),
        "{outcome:?}"
    );
    let seen = model.seen.borrow();
    let answers: Vec<&String> = seen[1]
        .1
        .iter()
        .filter_map(|m| match m {
            Message::Tool { content, .. } => Some(content),
            _ => None,
        })
        .collect();
    assert!(
        answers[0].contains("takes one argument, `rust_source`"),
        "{}",
        answers[0]
    );
    assert!(
        answers[1].contains("there is no tool `bash`"),
        "{}",
        answers[1]
    );
    assert!(answers[2].contains("not a JSON object"), "{}", answers[2]);
}

#[tokio::test]
async fn outages_use_no_attempt_and_a_lasting_one_leaves_the_chunk_unfailed() {
    let d = dirs();
    let down = |why: &str| Err(ModelError::Unavailable(why.into()));
    let model = script([
        down("connection refused"),
        down("HTTP 429"),
        submit("s1", GOOD),
    ]);
    let v = verdicts([Verdict::Unvalidated("ok".into())]);
    let Outcome::Done { attempts, log, .. } = run(&model, &v, &budget(1), &d).await else {
        panic!("expected Done");
    };
    assert_eq!(attempts, 1);
    assert!(log.contains("outage 2 (base): HTTP 429"), "{log}");

    let model = script((0..=OUTAGES).map(|_| down("OPENROUTER_API_KEY is not set")));
    let outcome = run(&model, &verdicts([]), &budget(3), &d).await;
    assert!(
        matches!(outcome, Outcome::Unavailable { .. }),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn failed_requests_count_against_the_chunk() {
    let d = dirs();
    let model = script([
        Err(ModelError::Reply("HTTP 400: context too long".into())),
        submit("s1", GOOD),
    ]);
    let v = verdicts([Verdict::Unvalidated("ok".into())]);
    let Outcome::Done { log, .. } = run(&model, &v, &budget(2), &d).await else {
        panic!("expected Done");
    };
    assert!(log.starts_with("turn 1 (base): HTTP 400"), "{log}");
}

#[test]
fn raw_newlines_inside_argument_strings_are_escaped() {
    let raw = "{\"rust_source\": \"fn a() {\n\t\\\"x\\\"\n}\"}";
    assert!(serde_json::from_str::<Value>(raw).is_err());
    let v: Value = serde_json::from_str(&escape_controls(raw)).unwrap();
    assert_eq!(v["rust_source"], "fn a() {\n\t\"x\"\n}");
    // Whitespace between tokens is left alone.
    assert_eq!(escape_controls("{\n\"a\": 1\n}"), "{\n\"a\": 1\n}");
}

#[test]
fn compaction_elides_the_oldest_tool_outputs_first() {
    let big = "x".repeat(60 << 10);
    let mut messages = vec![Message::System("s".into()), Message::User("task".into())];
    for i in 0..8 {
        messages.push(Message::Assistant(Reply {
            content: None,
            calls: vec![call(&i.to_string(), "read", json!({"path": "a"}))],
        }));
        messages.push(Message::Tool {
            call: i.to_string(),
            content: big.clone(),
        });
    }
    compact(&mut messages);
    let elided =
        |m: &Message| matches!(m, Message::Tool { content, .. } if content.starts_with("[elided"));
    assert!(elided(&messages[3]), "the oldest output goes first");
    assert!(!elided(messages.last().unwrap()), "the latest stays");
    assert_eq!(messages[1], Message::User("task".into()));
    let total: usize = messages
        .iter()
        .map(|m| match m {
            Message::Tool { content, .. } => content.len(),
            _ => 0,
        })
        .sum();
    assert!(total <= super::CONTEXT_BYTES, "{total}");
}
