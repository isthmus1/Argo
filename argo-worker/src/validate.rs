//! Judging a submitted rewrite: the seam for the compile/link/C-test pipeline (ARCHITECTURE.md
//! §5.8 steps 4–5, §5.9). A [`Validator`] judges one candidate; the agent loop turns a failure
//! into what `submit_rewrite` answers. [`BuildCheck`] is what exists so far: the candidate must
//! compile, fit the C program's symbols, link into it in place of the C definition, and export
//! the C function's ABI. Nothing runs the program yet, so a pass is still `Unvalidated`.

use std::collections::BTreeSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use argo_db_client::{CChunk, Origin};
use tokio::sync::Mutex;

use crate::abi;
use crate::program::{self, Build, Program, Toolchain};

/// What a claim's C program is built from: the ingest (its units) and this worker's clone of
/// its commit.
#[derive(Clone, Copy)]
pub struct Ingest<'a> {
    pub origin: &'a Origin,
    pub checkout: &'a Path,
}

/// One submitted candidate for a claimed chunk.
pub struct Candidate<'a> {
    pub chunk: &'a CChunk,
    /// Raw Rust that passed the shape check (`#[no_mangle] pub extern "C"`).
    pub source: &'a str,
    /// `source`, saved: what is compiled, so compiler errors name a file the agent can read.
    pub file: &'a Path,
    /// This submission's own directory, for build outputs.
    pub dir: &'a Path,
    /// The repository at the ingested commit; `None` when the ingest recorded no commit.
    /// Read-only: other readers share it and the worker refuses a clone that changed
    /// (`checkout::Checkout::sync`), so build in [`Candidate::dir`].
    pub checkout: Option<&'a Path>,
    /// The C program the candidate replaces a function of, built for this claim.
    pub program: Option<Arc<Program>>,
    /// The function the Rust must export: the C function's name, or a piece's symbol.
    pub symbol: &'a str,
    /// For a piece of a function: the C that replaces it (a call to `symbol`).
    pub c: Option<&'a str>,
}

/// What a validator established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Built, linked into the C program and its tests passed.
    Passed(String),
    /// Not (fully) checked: submitted as `Validation::Unvalidated`, with what was checked.
    Unvalidated(String),
    /// Rejected, for every reason found: the agent is told and the submission counts against
    /// the budget.
    Failed(Vec<Failure>),
}

/// One reason a candidate was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub stage: Stage,
    /// One line: what failed.
    pub summary: String,
    /// Specific causes, each actionable on its own.
    pub notes: Vec<String>,
    /// The full output behind it (compiler, linker), saved where the agent can read it.
    pub log: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Not the kind of source asked for (empty, markdown, no export).
    Shape,
    /// rustc rejected it.
    Compile,
    /// Its symbols can't fit the C program (not exported, redefines C, calls a `static`).
    Symbols,
    /// Linking it into the C program failed.
    Link,
    /// Its exported signature doesn't match the C function's ABI.
    Abi,
    /// A piece's C replacement doesn't compile in the piece's place.
    CCall,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Stage::Shape => "shape",
            Stage::Compile => "rustc",
            Stage::Symbols => "symbols",
            Stage::Link => "link",
            Stage::Abi => "abi",
            Stage::CCall => "c",
        }
    }
}

impl Failure {
    fn new(
        stage: Stage,
        summary: impl Into<String>,
        notes: Vec<String>,
        log: Option<String>,
    ) -> Failure {
        Failure {
            stage,
            summary: summary.into(),
            notes,
            log,
        }
    }

    pub fn shape(why: String) -> Failure {
        Failure::new(Stage::Shape, why, vec![], None)
    }
}

pub trait Validator {
    /// Per-claim setup before the first candidate, e.g. building the C program from `ingest`
    /// with this claim's files in `dir`; `None` when there is nothing to set up.
    fn prepare(
        &self,
        _chunk: &CChunk,
        _piece: Option<&crate::piece::Piece>,
        _ingest: Option<Ingest<'_>>,
        _dir: &Path,
    ) -> impl Future<Output = Option<Program>> {
        async { None }
    }

    /// Judge a candidate. Blocking work (compilers, test runs) belongs on a blocking thread.
    /// The future need not be `Send`: [`crate::work::Worker`] runs on its caller's task.
    fn validate(&self, candidate: &Candidate<'_>) -> impl Future<Output = Verdict>;
}

/// The honest default without a pipeline: every candidate is reported unvalidated.
pub struct Unvalidated;

impl Validator for Unvalidated {
    async fn validate(&self, _: &Candidate<'_>) -> Verdict {
        Verdict::Unvalidated("not compiled, linked or tested: no validator is configured".into())
    }
}

/// Compiles each candidate with `rustc` as the static library it is linked in as, then, given
/// the C program: checks its symbols against the program's, links it in place of the C
/// definition (problems the program has without it are not held against it), and compares its
/// exported signature with the C function's debug info.
#[derive(Debug, Clone)]
pub struct BuildCheck {
    /// `None` without a C compiler: then only the compile check runs.
    tools: Option<Toolchain>,
    /// The program's units compiled for the latest ingest, shared by every worker in the
    /// process (clones share it): compiled once, by whichever claim needs it first.
    builds: Arc<Mutex<Option<Shared>>>,
}

/// An ingest, by locator, commit and the units it compiled.
type BuildKey = (String, String, Vec<String>);
/// The build of one ingest.
type Shared = (BuildKey, Arc<Build>);

impl BuildCheck {
    /// `rustc` is required (without it every candidate would fail and use up every chunk's
    /// attempts); a C compiler only enables the link and ABI checks, so its absence is a warning.
    pub fn detect() -> Result<(BuildCheck, Option<String>), String> {
        let libs = native_libs()?;
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
        match Command::new(&cc).arg("--version").output() {
            Ok(out) if out.status.success() => Ok((BuildCheck::with(Toolchain { cc, libs }), None)),
            _ => Ok((
                BuildCheck::compile_only(),
                Some(format!(
                    "cannot run the C compiler `{cc}`: candidates are compiled but not linked or \
                     ABI-checked"
                )),
            )),
        }
    }

    /// Only the compile check, as without a C compiler.
    pub fn compile_only() -> BuildCheck {
        BuildCheck {
            tools: None,
            builds: Arc::default(),
        }
    }

    pub fn with(tools: Toolchain) -> BuildCheck {
        BuildCheck {
            tools: Some(tools),
            builds: Arc::default(),
        }
    }

    /// The program's units compiled for `ingest`: the shared build when it is this ingest's,
    /// else a new one (which then becomes the shared one).
    async fn build(&self, ingest: Ingest<'_>, tools: &Toolchain) -> Result<Arc<Build>, String> {
        let commit = ingest
            .origin
            .commit
            .clone()
            .ok_or("the ingest has no commit")?;
        let units: Vec<String> = ingest
            .origin
            .units
            .iter()
            .filter(|u| u.skipped.is_none())
            .map(|u| u.path.clone())
            .collect();
        let key = (ingest.origin.locator.clone(), commit, units.clone());
        let mut shared = self.builds.lock().await;
        if let Some((held, build)) = &*shared
            && *held == key
        {
            return Ok(build.clone());
        }
        let (root, tools) = (ingest.checkout.to_path_buf(), tools.clone());
        let build = tokio::task::spawn_blocking(move || Build::new(&root, &units, &tools))
            .await
            .map_err(|e| format!("building the C program did not finish: {e}"))??;
        for (path, why) in build.failures() {
            eprintln!("argo-worker: C program: `{path}` is left out of the link: {why}");
        }
        let build = Arc::new(build);
        *shared = Some((key, build.clone()));
        Ok(build)
    }
}

impl Validator for BuildCheck {
    async fn prepare(
        &self,
        chunk: &CChunk,
        piece: Option<&crate::piece::Piece>,
        ingest: Option<Ingest<'_>>,
        dir: &Path,
    ) -> Option<Program> {
        let (tools, ingest) = (self.tools.clone()?, ingest?);
        let off = |why: String| Program {
            notes: vec![format!("link and ABI checks are off: {why}")],
            ..Program::default()
        };
        let build = match self.build(ingest, &tools).await {
            Ok(build) => build,
            Err(why) => return Some(off(why)),
        };
        let (chunk, piece, root, dir) = (
            chunk.clone(),
            piece.cloned(),
            ingest.checkout.to_path_buf(),
            dir.to_path_buf(),
        );
        let prepared = tokio::task::spawn_blocking(move || {
            Program::prepare(&chunk, piece.as_ref(), &build, &root, &dir, &tools)
        });
        Some(
            prepared
                .await
                .unwrap_or_else(|e| off(format!("preparing the C program did not finish: {e}"))),
        )
    }

    async fn validate(&self, candidate: &Candidate<'_>) -> Verdict {
        let job = Job {
            name: candidate.symbol.to_string(),
            c: candidate.c.map(str::to_string),
            file: candidate.file.to_path_buf(),
            dir: candidate.dir.to_path_buf(),
            program: candidate.program.clone(),
            tools: self.tools.clone(),
        };
        // rustc and the linker take a while; off this task, so the claim's heartbeat keeps
        // running.
        match tokio::task::spawn_blocking(move || job.run()).await {
            Ok(verdict) => verdict,
            Err(e) => Verdict::Failed(vec![Failure::new(
                Stage::Compile,
                format!("the build did not finish: {e}"),
                vec![],
                None,
            )]),
        }
    }
}

/// One candidate's build, owned so it can run on a blocking thread.
struct Job {
    name: String,
    /// A piece's C replacement.
    c: Option<String>,
    file: PathBuf,
    dir: PathBuf,
    program: Option<Arc<Program>>,
    tools: Option<Toolchain>,
}

impl Job {
    fn run(self) -> Verdict {
        let lib = self.dir.join("libsnippet.a");
        if let Err(log) = rustc(&self.file, &lib) {
            let errors: Vec<String> = log
                .lines()
                .filter(|l| l.starts_with("error"))
                .filter(|l| !l.starts_with("error: aborting"))
                .take(8)
                .map(str::to_string)
                .collect();
            return Verdict::Failed(vec![Failure::new(
                Stage::Compile,
                "rustc rejected the code",
                errors,
                Some(log),
            )]);
        }
        let mut done = vec!["compiled with rustc".to_string()];
        let (Some(program), Some(tools)) = (&self.program, &self.tools) else {
            done.push("not linked or ABI-checked: no C program to check against".into());
            return Verdict::Unvalidated(done.join("; "));
        };
        let archive = std::fs::read(&lib).unwrap_or_default();
        let objects = own_objects(&archive);
        if objects.is_empty() {
            done.push(
                "not linked or ABI-checked: the candidate's objects weren't found in its library"
                    .into(),
            );
            return Verdict::Unvalidated(done.join("; "));
        }
        let rust = symbols(&objects);
        let mut failures = Vec::new();

        // Symbols: explained precisely here, and not repeated by the link below.
        let mut explained = BTreeSet::new();
        let mut notes = Vec::new();
        if !rust.global.contains(&self.name) {
            explained.insert(self.name.clone());
            notes.push(format!(
                "`{0}` is not exported: define it at the top level of the file as \
                 `#[no_mangle] pub extern \"C\" fn {0}`",
                self.name
            ));
        }
        if !program.symbols.global.is_empty() || !program.symbols.local.is_empty() {
            let mut redefined: Vec<&String> = rust
                .global
                .iter()
                .filter(|s| **s != self.name && program.symbols.global.contains(*s))
                .collect();
            redefined.sort();
            for s in redefined {
                explained.insert(s.clone());
                notes.push(format!(
                    "your Rust also defines `{s}`, which the C program already defines: declare \
                     it in an `extern \"C\"` block and call the C one instead of redefining it"
                ));
            }
            let mut statics: Vec<&String> = rust
                .undefined
                .iter()
                .filter(|s| {
                    program.symbols.local.contains(*s) && !program.symbols.global.contains(*s)
                })
                .collect();
            statics.sort();
            for s in statics {
                explained.insert(s.clone());
                notes.push(format!(
                    "`{s}` is `static` in the C program, so it has no linker symbol and Rust \
                     cannot call it through `extern \"C\"`: port its logic into this file as a \
                     private Rust fn (no `#[no_mangle]`)"
                ));
            }
        }
        if !notes.is_empty() {
            failures.push(Failure::new(
                Stage::Symbols,
                "the Rust's symbols don't fit the C program",
                notes,
                None,
            ));
        }

        // A piece: its C replacement spliced into its unit, which is compiled for this candidate;
        // the link and the ABI are then this unit's, with the other units.
        let mut c_signature = program.signature.clone();
        let mut declaration = program.declaration.clone();
        let linked = match &program.site {
            None => program.link(tools, &self.dir, &lib),
            Some(site) => {
                let Some(c) = &self.c else {
                    failures.push(Failure::new(
                        Stage::CCall,
                        "no `c_replacement`: give the C that replaces the TARGET lines",
                        vec![],
                        None,
                    ));
                    return Verdict::Failed(failures);
                };
                let replaced = format!("{}{c}{}", &site.text[..site.lo], &site.text[site.hi..]);
                let (unit, object) = (self.dir.join("unit.c"), self.dir.join("unit.o"));
                if let Err(log) = program::compile_text(tools, &unit, &replaced, &object, &[]) {
                    let errors: Vec<String> =
                        log.lines().filter(|l| l.contains("error")).take(8).map(str::to_string).collect();
                    failures.push(Failure::new(
                        Stage::CCall,
                        format!(
                            "the C replacement doesn't compile in the piece's place (the whole unit \
                             with it is saved at {})",
                            unit.display()
                        ),
                        errors,
                        Some(log),
                    ));
                    return Verdict::Failed(failures);
                }
                done.push("the C replacement compiled in place".into());
                let bytes = std::fs::read(&object).unwrap_or_default();
                if let Ok(Some(sig)) = abi::signature(&bytes, &self.name) {
                    declaration = format!("{}{}", self.name, sig.show());
                    c_signature = Some(sig);
                }
                let objects: Vec<PathBuf> = site.others.iter().cloned().chain([object]).collect();
                Some(program::link(tools, &self.dir, &objects, Some(&lib)))
            }
        };

        // Link, against what the C program alone already reports.
        match linked {
            None => done.push("not linked: no linkable C program".into()),
            Some((linked, log)) => {
                let base = &program.baseline;
                let mut notes = Vec::new();
                let new: Vec<&String> = linked
                    .undefined
                    .difference(&base.undefined)
                    .chain(linked.duplicate.difference(&base.duplicate))
                    .collect();
                for s in linked.undefined.difference(&base.undefined) {
                    if !explained.contains(s) {
                        notes.push(format!(
                            "`{s}` is undefined: neither the C program nor the system libraries \
                             define it. Check its exact name in the C source with grep, or \
                             implement it in Rust"
                        ));
                    }
                }
                for s in linked.duplicate.difference(&base.duplicate) {
                    if !explained.contains(s) {
                        notes.push(format!(
                            "`{s}` is defined twice: by your Rust and by the C program or a \
                             library it links"
                        ));
                    }
                }
                let still_undefined = linked.undefined.contains(&self.name);
                if linked.failed && notes.is_empty() && !base.failed && !still_undefined {
                    notes.push("the link failed; the log says why".into());
                }
                if !notes.is_empty() {
                    failures.push(Failure::new(
                        Stage::Link,
                        "linking the Rust into the C program in place of the C definition failed",
                        notes,
                        Some(yours_first(&log, &new, base.failed)),
                    ));
                } else if base.failed {
                    done.push(format!(
                        "linked into the C program (adding no link errors to the {} it has \
                         without the Rust)",
                        base.undefined.len() + base.duplicate.len() + base.other.len()
                    ));
                } else {
                    done.push("linked into the C program".into());
                }
            }
        }

        // ABI: what each compiler says the signature is.
        match (&c_signature, signature(&objects, &self.name)) {
            (Some(c), Ok(Some(r))) => {
                let diffs = abi::compare(c, &r);
                if diffs.is_empty() {
                    done.push(format!(
                        "exported signature matches the C ABI ({} parameters)",
                        c.params.len()
                    ));
                } else {
                    let log = format!(
                        "C declaration:  {}\nC, as compiled:    {}{}\nRust, as compiled: {}{}\n\n{}\n",
                        declaration,
                        self.name,
                        c.show(),
                        self.name,
                        r.show(),
                        diffs.join("\n")
                    );
                    failures.push(Failure::new(
                        Stage::Abi,
                        format!(
                            "the exported Rust signature doesn't match the C function's ABI; \
                             the C declaration is `{}`",
                            declaration
                        ),
                        diffs,
                        Some(log),
                    ));
                }
            }
            (Some(_), Ok(None)) => {} // Not exported: the symbol check says so.
            (Some(_), Err(e)) => done.push(format!("ABI not checked: {e}")),
            (None, _) => done.push("ABI not checked: no C signature".into()),
        }

        if failures.is_empty() {
            done.push("not run or tested".into());
            done.extend(program.notes.iter().cloned());
            Verdict::Unvalidated(done.join("; "))
        } else {
            Verdict::Failed(failures)
        }
    }
}

/// The linker's output with the lines about `symbols` (the candidate's problems) first: the
/// rest may be the C program's own problems, which would otherwise bury them.
fn yours_first(log: &str, symbols: &[&String], noisy: bool) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let mut yours = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        // GNU ld and lld quote the C name; macOS's ld quotes the Mach-O one (`"_name"`).
        let names = symbols.iter().any(|s| {
            [("`", "'"), ("'", "'"), ("‘", "’"), ("\"_", "\""), ("'_", "'")]
                .iter()
                .any(|(open, close)| line.contains(&format!("{open}{s}{close}")))
                || line.ends_with(&format!(": {s}"))
        });
        if !names {
            continue;
        }
        if i > 0 && lines[i - 1].contains("in function") && yours.last() != Some(&lines[i - 1]) {
            yours.push(lines[i - 1]);
        }
        yours.push(line);
    }
    let rest = if noisy {
        "The full linker output, which also shows the C program's own link problems (they are \
         there without your Rust too, and are not yours to fix):"
    } else {
        "The full linker output:"
    };
    format!(
        "Lines about your Rust:\n{}\n\n{rest}\n{log}",
        yours.join("\n")
    )
}

/// Compile `file` as the static library it is linked in as, with debug info for the ABI check.
fn rustc(file: &Path, lib: &Path) -> Result<(), String> {
    let out = Command::new("rustc")
        .args([
            "--edition=2021",
            "--crate-type=staticlib",
            "--crate-name=snippet",
            "-g",
            "-Ccodegen-units=1",
            "-Copt-level=0",
            "--color=never",
            "-o",
        ])
        .arg(lib)
        .arg(file)
        .output()
        .map_err(|e| format!("cannot run rustc: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// The candidate's own objects in its static library (the rest is Rust's standard library).
fn own_objects(archive: &[u8]) -> Vec<&[u8]> {
    let Ok(file) = object::read::archive::ArchiveFile::parse(archive) else {
        return vec![];
    };
    file.members()
        .filter_map(Result::ok)
        .filter(|m| {
            let name = m.name();
            name.starts_with(b"snippet.") || name.starts_with(b"libsnippet.")
        })
        .filter_map(|m| m.data(archive).ok())
        .collect()
}

/// The symbols of the candidate's objects together: what one defines another doesn't need.
fn symbols(objects: &[&[u8]]) -> program::Symbols {
    let mut all = program::Symbols::default();
    for bytes in objects {
        let s = program::symbols(bytes).unwrap_or_default();
        all.global.extend(s.global);
        all.local.extend(s.local);
        all.undefined.extend(s.undefined);
    }
    let defined: Vec<String> = all.global.iter().chain(&all.local).cloned().collect();
    for s in defined {
        all.undefined.remove(&s);
    }
    all
}

/// The export's signature from whichever of the candidate's objects describes it.
fn signature(objects: &[&[u8]], name: &str) -> Result<Option<abi::Signature>, String> {
    for bytes in objects {
        if let Some(sig) = abi::signature(bytes, name)? {
            return Ok(Some(sig));
        }
    }
    Ok(None)
}

/// The native libraries a Rust static library needs at link time; also proves rustc runs.
fn native_libs() -> Result<Vec<String>, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let src = dir.path().join("empty.rs");
    std::fs::write(&src, "").map_err(|e| e.to_string())?;
    let out = Command::new("rustc")
        .args(["--crate-type=staticlib", "--print=native-static-libs", "-o"])
        .arg(dir.path().join("libempty.a"))
        .arg(&src)
        .output()
        .map_err(|e| format!("cannot run rustc: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "rustc failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Ok(stderr
        .lines()
        .find_map(|l| l.split_once("native-static-libs:").map(|(_, libs)| libs))
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests;
