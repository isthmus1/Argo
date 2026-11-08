//! The agent loop: a conversation with the model, which may call `read`, `grep` and `glob`
//! (read-only, see [`crate::tools`]) as often as it likes and `submit_rewrite` to have a
//! complete Rust file checked. An accepted submission ends the loop with the result; a rejected
//! one is answered with what failed, why, and where the full logs are, and the model carries on
//! in the same conversation, so it always sees its own earlier code and every earlier failure.
//!
//! Budgets: `max_attempts` rejected submissions (and failed requests), and `max_turns` model
//! turns in all, every one with the same model. An unavailable endpoint is waited out instead,
//! and if it stays down the chunk is left to its lease, never failed.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use argo_db_client::{CChunk, Validation};
use serde_json::{Value, json};
use tokio::time::sleep;

use crate::log::{MAX_LOG_BYTES, tail};
use crate::model::{Message, Model, ModelError, ToolCall, ToolSpec};
use crate::program::Program;
use crate::tools::{Tools, cut};
use crate::validate::{Candidate, Failure, Validator, Verdict};
use crate::{prompt, shape};

mod budget;

pub use budget::Budget;
use budget::OUTAGES;

/// The model's request is answered with this when it called no tool.
const NUDGE: &str = "You must call a tool. Use read, grep or glob to look at the code, or call \
    submit_rewrite with the complete raw Rust source. Text replies do nothing.";
/// Above this many bytes of conversation, the oldest tool outputs are elided.
const CONTEXT_BYTES: usize = 240 << 10;
/// How much of each log a rejection quotes; the rest is read from the file.
const EXCERPT_BYTES: usize = 3 << 10;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Never `Validation::Failed`: a failing candidate is rejected, not submitted.
    Done {
        source: String,
        model: String,
        /// Submissions made, the accepted one included.
        attempts: u32,
        validation: Validation,
        log: String,
    },
    Failed {
        attempts: u32,
        log: String,
    },
    /// The endpoint stayed unavailable: nothing is recorded, the claim lapses with its lease.
    Unavailable {
        log: String,
    },
}

/// What the agent works on.
pub struct Task<'a> {
    pub chunk: &'a CChunk,
    /// The clone at the ingested commit, if there is one.
    pub checkout: Option<&'a Path>,
    /// The claim's own directory: the C program, and each submission's source and logs.
    pub session: &'a Path,
    pub program: Option<Arc<Program>>,
}

/// Translate the task's chunk.
pub async fn translate(
    model: &impl Model,
    validator: &impl Validator,
    budget: &Budget,
    task: &Task<'_>,
) -> Outcome {
    let tools = match Tools::new(task.checkout, task.session) {
        Ok(tools) => tools,
        Err(why) => {
            return Outcome::Failed {
                attempts: 0,
                log: format!("the agent's tools could not be set up: {why}\n"),
            };
        }
    };
    let mut specs = Tools::specs();
    specs.push(submit_spec());
    let mut messages = prompt::conversation(task, budget);
    let mut log = String::new();
    let (mut failures, mut outages, mut turns, mut submissions) = (0u32, 0u32, 0u32, 0u32);
    let name = budget.model.as_str();
    'turns: while failures < budget.max_attempts {
        if turns >= budget.max_turns {
            let _ = writeln!(
                log,
                "gave up after {turns} turns without an accepted submission"
            );
            break;
        }
        compact(&mut messages);
        let reply = match model.chat(name, &messages, &specs).await {
            Ok(reply) => reply,
            Err(ModelError::Unavailable(why)) => {
                outages += 1;
                let _ = writeln!(log, "outage {outages} ({name}): {why}");
                if outages > OUTAGES {
                    return Outcome::Unavailable {
                        log: tail(&log, MAX_LOG_BYTES),
                    };
                }
                sleep(budget.pause(outages)).await;
                continue;
            }
            Err(ModelError::Reply(why)) => {
                (outages, turns, failures) = (0, turns + 1, failures + 1);
                let _ = writeln!(log, "turn {turns} ({name}): {why}");
                continue;
            }
        };
        (outages, turns) = (0, turns + 1);
        messages.push(Message::Assistant(reply.clone()));
        if reply.calls.is_empty() {
            let _ = writeln!(log, "turn {turns} ({name}): no tool call");
            messages.push(Message::User(NUDGE.into()));
            continue;
        }
        for call in &reply.calls {
            let content = match call.name.as_str() {
                "submit_rewrite" => {
                    let source = match arguments(call).and_then(|a| rust_source(&a)) {
                        Ok(source) => source,
                        Err(why) => {
                            messages.push(Message::Tool {
                                call: call.id.clone(),
                                content: why,
                            });
                            continue;
                        }
                    };
                    submissions += 1;
                    match submit(validator, task, submissions, &source).await {
                        Ok((validation, checked)) => {
                            let _ = writeln!(
                                log,
                                "submission {submissions} ({name}): accepted: {checked}"
                            );
                            return Outcome::Done {
                                source,
                                model: name.to_string(),
                                attempts: submissions,
                                validation,
                                log: tail(&log, MAX_LOG_BYTES),
                            };
                        }
                        Err(rejected) => {
                            failures += 1;
                            let summary: Vec<String> = rejected
                                .iter()
                                .map(|f| format!("{}: {}", f.stage.name(), f.summary))
                                .collect();
                            let _ = writeln!(
                                log,
                                "submission {submissions} ({name}): {}",
                                summary.join("; ")
                            );
                            let left = budget.max_attempts.saturating_sub(failures);
                            let content = report(task.session, submissions, left, &rejected);
                            messages.push(Message::Tool {
                                call: call.id.clone(),
                                content,
                            });
                            if left == 0 {
                                break 'turns;
                            }
                            continue;
                        }
                    }
                }
                name if Tools::has(name) => match arguments(call) {
                    Ok(args) => {
                        let (tools, name) = (tools.clone(), name.to_string());
                        // Walking a large tree takes a while; off this task, so the claim's
                        // heartbeat (and other workers on it) keep running.
                        tokio::task::spawn_blocking(move || tools.run(&name, &args))
                            .await
                            .unwrap_or_else(|e| format!("error: the tool did not finish: {e}"))
                    }
                    Err(why) => why,
                },
                other => format!(
                    "error: there is no tool `{other}`; the tools are read, grep, glob and \
                     submit_rewrite"
                ),
            };
            messages.push(Message::Tool {
                call: call.id.clone(),
                content,
            });
        }
    }
    Outcome::Failed {
        attempts: submissions,
        log: tail(&log, MAX_LOG_BYTES),
    }
}

/// `submit_rewrite`: the one tool that builds code. Its schema and description are the only
/// contract for the source's form, so they insist on the raw file.
pub fn submit_spec() -> ToolSpec {
    ToolSpec {
        name: "submit_rewrite",
        description: "Submit your complete Rust rewrite of the TARGET for checking. \
            `rust_source` is the raw text of a .rs file, exactly as it would be saved to disk: \
            Rust code only, with no markdown, no ``` fences and no prose except // comments. It \
            is compiled with rustc as a static library, checked against the C program's symbols, \
            linked into the C program in place of the C definition, and its exported signature \
            is compared with the C function's ABI. If every check passes, the rewrite is \
            accepted and your task is over. If not, you get back which checks failed, why, and \
            the paths of the full logs (open them with read); fix the code and submit the whole \
            corrected file again.",
        parameters: json!({
            "type": "object",
            "properties": {
                "rust_source": {
                    "type": "string",
                    "description": "The complete raw Rust source file as plain text. It must \
                        start with Rust (a comment, an attribute, `use`, `extern` or an item), \
                        never with ``` or prose, and contain no markdown anywhere."
                }
            },
            "required": ["rust_source"],
            "additionalProperties": false
        }),
    }
}

/// Check one submission: saved, shape-checked, then validated. The validation and what was
/// checked, or every reason it was rejected.
async fn submit(
    validator: &impl Validator,
    task: &Task<'_>,
    n: u32,
    source: &str,
) -> Result<(Validation, String), Vec<Failure>> {
    let dir = task.session.join(format!("submission-{n}"));
    let file = dir.join("lib.rs");
    let saved = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&file, source));
    if let Err(e) = saved {
        return Err(vec![Failure::shape(format!(
            "the worker could not save the submission ({e}); submit again"
        ))]);
    }
    shape::check(task.chunk, source).map_err(|why| vec![Failure::shape(why)])?;
    let candidate = Candidate {
        chunk: task.chunk,
        source,
        file: &file,
        dir: &dir,
        checkout: task.checkout,
        program: task.program.clone(),
    };
    match validator.validate(&candidate).await {
        Verdict::Passed(log) => Ok((Validation::Passed, log)),
        Verdict::Unvalidated(log) => Ok((Validation::Unvalidated, log)),
        Verdict::Failed(failures) => Err(failures),
    }
}

/// What a rejected submission is answered with. Each failure's full log is saved beside the
/// submission, and its start quoted, since the first errors are the ones to fix.
fn report(session: &Path, n: u32, left: u32, failures: &[Failure]) -> String {
    let dir = session.join(format!("submission-{n}"));
    let mut out = format!(
        "REJECTED: submission {n}. Your source is saved at {}.\n",
        dir.join("lib.rs").display()
    );
    for f in failures {
        let _ = writeln!(out, "\n[{}] {}", f.stage.name(), f.summary);
        for note in &f.notes {
            let _ = writeln!(out, "  - {note}");
        }
        if let Some(text) = &f.log {
            let path = dir.join(format!("{}.log", f.stage.name()));
            match std::fs::write(&path, text) {
                Ok(()) => {
                    let _ = writeln!(out, "  full log: {}", path.display());
                }
                Err(e) => {
                    let _ = writeln!(out, "  (the log could not be saved: {e})");
                }
            }
            let _ = writeln!(out, "  log start:\n{}", indent(&head(text, EXCERPT_BYTES)));
        }
    }
    if left == 0 {
        out.push_str("\nNo submissions are left: this chunk has failed.\n");
    } else {
        let _ = write!(
            out,
            "\nFix every cause above, then call submit_rewrite with the whole corrected file. \
             {left} more rejection(s) and this chunk fails.\n"
        );
    }
    cut(out, 48 << 10)
}

/// The first `max` bytes of `text` (whole characters), saying if more was cut.
fn head(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.trim_end().to_string();
    }
    let mut at = max;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    format!(
        "{}\n[{} more bytes in the full log]",
        &text[..at],
        text.len() - at
    )
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A call's arguments as a JSON object, or the error to answer it with.
fn arguments(call: &ToolCall) -> Result<Value, String> {
    let parsed = serde_json::from_str::<Value>(&call.arguments)
        .or_else(|_| serde_json::from_str::<Value>(&escape_controls(&call.arguments)));
    match parsed {
        Ok(v @ Value::Object(_)) => Ok(v),
        Ok(_) | Err(_) => Err(format!(
            "error: the arguments to {} are not a JSON object: {}",
            call.name,
            cut(call.arguments.clone(), 300)
        )),
    }
}

fn rust_source(args: &Value) -> Result<String, String> {
    match args.get("rust_source") {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(
            "error: submit_rewrite takes one argument, `rust_source`: the complete raw \
                  Rust source as a string. Nothing was submitted."
                .into(),
        ),
    }
}

/// Raw newlines and tabs inside JSON strings escaped: some servers pass code through unescaped.
fn escape_controls(json: &str) -> String {
    let (mut out, mut in_string, mut escaped) = (String::with_capacity(json.len()), false, false);
    for c in json.chars() {
        match c {
            _ if escaped => {
                escaped = false;
                out.push(c);
            }
            '\\' if in_string => {
                escaped = true;
                out.push(c);
            }
            '"' => {
                in_string = !in_string;
                out.push(c);
            }
            '\n' if in_string => out.push_str("\\n"),
            '\r' if in_string => out.push_str("\\r"),
            '\t' if in_string => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

const ELIDED: &str = "[elided to save context; call the tool again if you need it]";

/// Keep the conversation under [`CONTEXT_BYTES`]: the oldest tool outputs, and the arguments of
/// the oldest calls (earlier submissions' code, saved on disk anyway), are elided first. The
/// system prompt, the task and the latest exchanges stay whole.
fn compact(messages: &mut [Message]) {
    let size = |m: &Message| match m {
        Message::System(t) | Message::User(t) => t.len(),
        Message::Tool { content, .. } => content.len(),
        Message::Assistant(r) => {
            r.content.as_ref().map_or(0, String::len)
                + r.calls.iter().map(|c| c.arguments.len()).sum::<usize>()
        }
    };
    let mut total: usize = messages.iter().map(size).sum();
    let keep = messages.len().saturating_sub(6);
    for m in messages.iter_mut().take(keep).skip(2) {
        if total <= CONTEXT_BYTES {
            return;
        }
        match m {
            Message::Tool { content, .. } if content.len() > ELIDED.len() => {
                total = total - content.len() + ELIDED.len();
                *content = ELIDED.into();
            }
            Message::Assistant(r) => {
                for c in &mut r.calls {
                    let elided = json!({ "elided": ELIDED }).to_string();
                    if c.arguments.len() > elided.len() {
                        total = total - c.arguments.len() + elided.len();
                        c.arguments = elided;
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
