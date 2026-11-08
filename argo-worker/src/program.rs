//! The C program a candidate is linked into, built the way the project builds: one object per
//! translation unit the server ingested, compiled from the worker's clone with the Makefile's
//! flags. (The ingested text itself is every unit `#include`d into one, which the parser reads
//! but a compiler needn't accept: two units may define the same typedef differently.) Every
//! output goes to a directory of the worker's own: the clone is shared by every reader and the
//! worker refuses one that changed (`checkout::Checkout::sync`).
//!
//! [`Build`] compiles every unit once per ingest. [`Program`] is one claim's view: the target's
//! unit preprocessed, its definition found by the chunk's exact text and swapped for its
//! declaration, and the result linked with every other unit. From it come the target's C
//! signature (its debug info, see [`crate::abi`]), the program's symbols (which functions are
//! `static`, so Rust can't link to them), and the baseline: what linking the program reports
//! without any Rust (several `main`s, an optional library not installed), which a candidate is
//! not held to.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use argo_c::project::cpp::DIALECT_FLAGS;
use argo_c::project::lines::Mapped;
use argo_c::project::make;
use argo_db_client::{CChunk, ChunkKind};
use object::{BinaryFormat, Object, ObjectSymbol, SymbolKind};
use regex::Regex;
use tempfile::TempDir;

use crate::abi::{self, Signature};

/// Specifiers dropped from the declaration that replaces the definition: the Rust export is an
/// external symbol, so the C side must not expect a `static` or inline one.
const SPECIFIERS: &[&str] = &["static", "inline", "__inline", "__inline__", "extern"];

/// How the C program is built and linked: the C compiler, and the native libraries a Rust
/// static library needs (`rustc --print native-static-libs`).
#[derive(Debug, Clone)]
pub struct Toolchain {
    pub cc: String,
    pub libs: Vec<String>,
}

/// Every unit of the program, compiled once per ingest.
#[derive(Debug)]
pub struct Build {
    dir: TempDir,
    units: Vec<Unit>,
}

#[derive(Debug)]
struct Unit {
    /// Relative to the repository root.
    path: String,
    /// Whether it compiled only with the Makefile's optional features turned off.
    without_optional: bool,
    object: Option<PathBuf>,
    symbols: Symbols,
    /// Why it didn't compile.
    error: Option<String>,
    /// Preprocessed with the flags it compiled with, line markers removed: where a chunk's
    /// exact text is looked for.
    text: Option<String>,
}

/// One claim's program: the target's unit with the definition replaced, and every other unit.
#[derive(Debug, Default)]
pub struct Program {
    /// The C symbol the Rust replaces.
    pub name: String,
    /// The declaration left where the definition was: what the Rust must implement.
    pub declaration: String,
    /// The target's unit, preprocessed: every declaration the target can see.
    pub preprocessed: Option<PathBuf>,
    /// The objects a candidate is linked with; empty when the program couldn't be built.
    pub objects: Vec<PathBuf>,
    /// What linking `objects` without any Rust reports.
    pub baseline: Problems,
    /// The original program's symbols, every unit's together.
    pub symbols: Symbols,
    /// The target as the C compiler compiled it.
    pub signature: Option<Signature>,
    /// Checks that can't run for this chunk, and why.
    pub notes: Vec<String>,
    /// For a piece of a function: where it sits, to splice each candidate's C call into.
    pub site: Option<Site>,
}

/// Where a piece sits in its function's unit. Each candidate's C replacement is spliced in at
/// `lo..hi` and that unit compiled, then linked with `others` and the Rust.
#[derive(Debug, Default, Clone)]
pub struct Site {
    /// The unit's preprocessed text, as compiled here.
    pub text: String,
    /// The piece's byte range in `text`.
    pub lo: usize,
    pub hi: usize,
    /// Every other unit's object.
    pub others: Vec<PathBuf>,
    /// The unit's file, relative to `root`, and the clone it is in: the test oracle puts the
    /// spliced unit there in a copy of the clone and builds it with the project's own build.
    pub path: String,
    pub root: PathBuf,
}

/// Symbols of one or more objects, by binding.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Symbols {
    pub global: HashSet<String>,
    /// `static` functions and variables: no other object can link to them.
    pub local: HashSet<String>,
    /// Referenced but not defined.
    pub undefined: HashSet<String>,
}

impl Symbols {
    fn extend(&mut self, other: &Symbols) {
        self.global.extend(other.global.iter().cloned());
        self.local.extend(other.local.iter().cloned());
        self.undefined.extend(other.undefined.iter().cloned());
    }
}

/// What a link reported, by symbol.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Problems {
    pub failed: bool,
    pub undefined: BTreeSet<String>,
    pub duplicate: BTreeSet<String>,
    /// Error lines naming no symbol.
    pub other: BTreeSet<String>,
}

/// The flags `path` (relative to `root`) is compiled with, as the server read them (after
/// `cpp::DIALECT_FLAGS`, which every `cc` call here passes first): the repository as an include
/// directory, the Makefile's shared flags and the file's own rule's,
/// and `__FILE__` relative to the root (as the server ingested it).
fn flags(
    root: &Path,
    makefile: &make::Makefile,
    path: &str,
    without_optional: bool,
) -> Vec<String> {
    let mut flags = vec![format!("-I{}", root.display())];
    flags.extend(makefile.flags.iter().cloned());
    flags.extend(makefile.flags_for(&root.join(path)).iter().cloned());
    if without_optional {
        for d in &makefile.optional {
            let name = d.get(2..).unwrap_or("").split('=').next().unwrap_or("");
            flags.push(format!("-U{name}"));
        }
    }
    flags.push(format!("-fmacro-prefix-map={}/=", root.display()));
    flags
}

impl Build {
    /// Compile each of `units` (paths relative to `root`, the clone) into a new temporary
    /// directory, in parallel. A unit that doesn't compile is left out of the link.
    pub fn new(root: &Path, units: &[String], tools: &Toolchain) -> Result<Build, String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("{}: {e}", root.display()))?;
        let dir = tempfile::Builder::new()
            .prefix("argo-c-build-")
            .tempdir()
            .map_err(|e| format!("cannot make a build directory: {e}"))?;
        let makefile = make::read(&root);
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let per = units.len().div_ceil(threads).max(1);
        let mut built: Vec<Unit> = Vec::with_capacity(units.len());
        std::thread::scope(|scope| {
            let jobs: Vec<_> = units
                .chunks(per)
                .enumerate()
                .map(|(c, paths)| {
                    let (root, makefile, dir) = (&root, &makefile, dir.path());
                    scope.spawn(move || {
                        paths
                            .iter()
                            .enumerate()
                            .map(|(k, path)| {
                                let out = dir.join(format!("unit-{}.o", c * per + k));
                                compile_unit(root, makefile, path, &out, tools)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for job in jobs {
                built.extend(job.join().unwrap_or_default());
            }
        });
        Ok(Build { dir, units: built })
    }

    /// Units that didn't compile, and why.
    pub fn failures(&self) -> impl Iterator<Item = (&str, &str)> {
        self.units
            .iter()
            .filter_map(|u| Some((u.path.as_str(), u.error.as_deref()?)))
    }

    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// The unit `chunk` is defined in, that unit's preprocessed text, and where in it the
    /// chunk's exact text is. A chunk whose file is a header (a `static
    /// inline` helper, or a template a unit includes with macros set) is placed in the first
    /// unit holding its text exactly once.
    /// `chunk`'s unit (its path, relative to the clone), that unit's preprocessed text, and
    /// where `chunk` starts in it: where `argo export --mixed` splices.
    pub fn locate(
        &self,
        chunk: &CChunk,
        root: &Path,
        makefile: &make::Makefile,
        tools: &Toolchain,
    ) -> Result<(String, String, usize), String> {
        self.place(chunk, root, makefile, tools).map(|(u, text, at)| (u.path.clone(), text, at))
    }

    fn place(
        &self,
        chunk: &CChunk,
        root: &Path,
        makefile: &make::Makefile,
        tools: &Toolchain,
    ) -> Result<(&Unit, String, usize), String> {
        let once = |text: &str| -> Result<usize, usize> {
            let found: Vec<usize> = text
                .match_indices(&chunk.source)
                .map(|(at, _)| at)
                .collect();
            match found[..] {
                [lo] => Ok(lo),
                _ => Err(found.len()),
            }
        };
        if let Some(unit) = self.units.iter().find(|u| u.path == chunk.path) {
            if let Some(e) = &unit.error {
                return Err(format!("`{}` doesn't compile here: {e}", unit.path));
            }
            let text = unit.text.as_deref().unwrap_or_default();
            let count = match once(text) {
                Ok(lo) => return Ok((unit, text.to_string(), lo)),
                Err(count) => count,
            };
            // The server may have read the unit with the optional features the other way.
            let flags = flags(root, makefile, &unit.path, !unit.without_optional);
            if !makefile.optional.is_empty()
                && let Ok(text) = preprocess(tools, &flags, &root.join(&unit.path))
                && let Ok(lo) = once(&text)
            {
                return Ok((unit, text, lo));
            }
            return Err(format!(
                "the chunk's text occurs {count} times in `{}` as preprocessed here",
                unit.path
            ));
        }
        for unit in self.units.iter().filter(|u| u.error.is_none()) {
            if let Some(text) = &unit.text
                && let Ok(lo) = once(text)
            {
                return Ok((unit, text.clone(), lo));
            }
        }
        Err(format!(
            "`{}` is not a translation unit, and no unit holds the chunk's text exactly once",
            chunk.path
        ))
    }
}

fn compile_unit(
    root: &Path,
    makefile: &make::Makefile,
    path: &str,
    out: &Path,
    tools: &Toolchain,
) -> Unit {
    let mut unit = Unit {
        path: path.to_string(),
        without_optional: false,
        object: None,
        symbols: Symbols::default(),
        error: None,
        text: None,
    };
    let source = root.join(path);
    let attempt =
        |without: bool| run_cc(tools, &flags(root, makefile, path, without), &source, out);
    let result = attempt(false).or_else(|first| {
        if makefile.optional.is_empty() {
            return Err(first);
        }
        unit.without_optional = true;
        attempt(true).map_err(|_| first)
    });
    match result {
        Ok(()) => {
            unit.symbols = std::fs::read(out)
                .ok()
                .and_then(|b| symbols(&b).ok())
                .unwrap_or_default();
            unit.object = Some(out.to_path_buf());
            let flags = flags(root, makefile, path, unit.without_optional);
            unit.text = preprocess(tools, &flags, &source).ok();
        }
        Err(e) => unit.error = Some(first_error(&e)),
    }
    unit
}

impl Program {
    /// The program for `chunk`, with its own files in `dir`. `root` is this worker's clone (the
    /// same commit `build` was compiled from). Never fails: a check that can't be set up is left
    /// out and said so in [`Program::notes`].
    pub fn prepare(
        chunk: &CChunk,
        piece: Option<&crate::piece::Piece>,
        build: &Build,
        root: &Path,
        dir: &Path,
        tools: &Toolchain,
    ) -> Program {
        if let Some(piece) = piece {
            return Program::prepare_piece(chunk, piece, build, root, dir, tools);
        }
        let mut p = Program {
            name: chunk.name.clone(),
            ..Program::default()
        };
        let off = |p: &mut Program, why: String| {
            p.notes.push(format!("link and ABI checks are off: {why}"));
        };
        if chunk.kind != ChunkKind::Function {
            off(&mut p, "not a function".into());
            return p;
        }
        let Some(declaration) = declaration(&chunk.source) else {
            off(&mut p, "the chunk is not a function definition".into());
            return p;
        };
        p.declaration = declaration;
        if let Err(e) = std::fs::create_dir_all(dir) {
            off(&mut p, format!("{}: {e}", dir.display()));
            return p;
        }
        let Ok(root) = root.canonicalize() else {
            off(&mut p, "the clone is missing".into());
            return p;
        };
        let makefile = make::read(&root);
        // The unit as the compiler sees it, and the definition in it by its exact text.
        let (target, text, lo) = match build.place(chunk, &root, &makefile, tools) {
            Ok(placed) => placed,
            Err(why) => {
                off(&mut p, why);
                return p;
            }
        };
        let hi = lo + chunk.source.len();
        let preprocessed = dir.join("original.c");

        // The original unit, with the target kept even if nothing calls it: its debug info has
        // the C signature, and its symbol table says what is `static`.
        let keep = format!(
            "\n__attribute__((used)) static void *const __argo_keep_target = (void *)&{};\n",
            chunk.name
        );
        let original = dir.join("original.o");
        if let Err(e) = compile_text(
            tools,
            &preprocessed,
            &format!("{text}{keep}"),
            &original,
            &[],
        ) {
            off(
                &mut p,
                format!(
                    "`{}` doesn't compile preprocessed: {}",
                    target.path,
                    first_error(&e)
                ),
            );
            return p;
        }
        p.preprocessed = Some(preprocessed);
        let bytes = std::fs::read(&original).unwrap_or_default();
        for u in build.units.iter().filter(|u| u.path != target.path) {
            p.symbols.extend(&u.symbols);
        }
        let mut own = symbols(&bytes).unwrap_or_default();
        own.local.remove("__argo_keep_target");
        p.symbols.extend(&own);
        match abi::signature(&bytes, &chunk.name) {
            Ok(Some(sig)) => p.signature = Some(sig),
            Ok(None) => p.notes.push(format!(
                "the ABI check is off: no debug info for `{}` in the C build",
                chunk.name
            )),
            Err(e) => p.notes.push(format!("the ABI check is off: {e}")),
        }

        // The unit the candidate is linked with. A `static` declaration elsewhere in the unit
        // may conflict with the external one; then every `static` is dropped from this unit
        // (the symbol checks still use the original bindings).
        let replaced = format!("{}{};{}", &text[..lo], p.declaration, &text[hi..]);
        let (source, object) = (dir.join("linked.c"), dir.join("linked.o"));
        let compiled = compile_text(tools, &source, &replaced, &object, &[]).or_else(|first| {
            compile_text(
                tools,
                &source,
                &replaced,
                &object,
                &["-Dstatic=", "-fgnu89-inline"],
            )
            .map(|()| {
                p.notes.push(
                    "the target's unit is linked with `static` dropped (a `static` \
                         declaration of the target conflicted with the external one)"
                        .into(),
                );
            })
            .map_err(|_| first)
        });
        if let Err(e) = compiled {
            p.notes.push(format!(
                "the link check is off: `{}` doesn't compile with the target replaced by its \
                 declaration: {}",
                target.path,
                first_error(&e)
            ));
            return p;
        }
        p.objects = build
            .units
            .iter()
            .filter(|u| u.path != target.path)
            .filter_map(|u| u.object.clone())
            .chain([object])
            .collect();
        p.baseline = link(tools, dir, &p.objects, None).0;
        p
    }

    /// The program for a piece of a function: its function's unit, where the piece sits in it,
    /// and the other units. The unit itself is compiled per candidate, with that candidate's C
    /// replacement spliced in at the piece.
    fn prepare_piece(
        chunk: &CChunk,
        piece: &crate::piece::Piece,
        build: &Build,
        root: &Path,
        dir: &Path,
        tools: &Toolchain,
    ) -> Program {
        let mut p = Program {
            name: piece.symbol.clone(),
            ..Program::default()
        };
        let off = |p: &mut Program, why: String| {
            p.notes.push(format!("link checks are off: {why}"));
        };
        if let Err(e) = std::fs::create_dir_all(dir) {
            off(&mut p, format!("{}: {e}", dir.display()));
            return p;
        }
        let Ok(root) = root.canonicalize() else {
            off(&mut p, "the clone is missing".into());
            return p;
        };
        let makefile = make::read(&root);
        let (target, text, function_at) = match build.place(&piece.function, &root, &makefile, tools)
        {
            Ok(placed) => placed,
            Err(why) => {
                off(&mut p, format!("the piece's function: {why}"));
                return p;
            }
        };
        // The piece inside its function: by the rows' byte ranges (both index the same ingested
        // text, contiguous inside the function), else its text found once in the function.
        let function = &piece.function.source;
        let by_range = chunk
            .byte_lo
            .checked_sub(piece.function.byte_lo)
            .map(|r| r as usize)
            .filter(|&r| function.get(r..r + chunk.source.len()) == Some(chunk.source.as_str()));
        let by_text = || {
            let found: Vec<usize> = function.match_indices(&chunk.source).map(|(at, _)| at).collect();
            (found.len() == 1).then(|| found[0])
        };
        let Some(within) = by_range.or_else(by_text) else {
            off(&mut p, "the piece's text isn't found in its function".into());
            return p;
        };
        let lo = function_at + within;
        let hi = lo + chunk.source.len();

        // The original unit: what the agent reads, its symbols, and the program's baseline.
        let (preprocessed, original) = (dir.join("original.c"), dir.join("original.o"));
        if let Err(e) = compile_text(tools, &preprocessed, &text, &original, &[]) {
            off(&mut p, format!("`{}` doesn't compile preprocessed: {}", target.path, first_error(&e)));
            return p;
        }
        p.preprocessed = Some(preprocessed);
        let others: Vec<PathBuf> = build
            .units
            .iter()
            .filter(|u| u.path != target.path)
            .filter_map(|u| u.object.clone())
            .collect();
        for u in build.units.iter().filter(|u| u.path != target.path) {
            p.symbols.extend(&u.symbols);
        }
        p.symbols.extend(&symbols(&std::fs::read(&original).unwrap_or_default()).unwrap_or_default());
        p.objects = others.iter().cloned().chain([original]).collect();
        p.baseline = link(tools, dir, &p.objects, None).0;
        let path = target.path.clone();
        p.site = Some(Site { text, lo, hi, others, path, root: root.clone() });
        p
    }

    /// Link `rust` (a static library) into the program, in `dir`; `None` when there is no
    /// program to link into.
    pub fn link(&self, tools: &Toolchain, dir: &Path, rust: &Path) -> Option<(Problems, String)> {
        (!self.objects.is_empty()).then(|| link(tools, dir, &self.objects, Some(rust)))
    }
}

/// The definition's header with its storage specifiers removed: `int add(int a, int b)`.
fn declaration(definition: &str) -> Option<String> {
    let (header, _) = definition.split_once('{')?;
    let mut header = header.trim();
    loop {
        let word = header.split_whitespace().next()?;
        if !SPECIFIERS.contains(&word) {
            break;
        }
        header = header[word.len()..].trim_start();
    }
    header.contains('(').then(|| header.to_string())
}

fn run_cc(tools: &Toolchain, flags: &[String], source: &Path, out: &Path) -> Result<(), String> {
    // The dialect the server preprocesses (and the parser reads) every unit in.
    let output = Command::new(&tools.cc)
        .args(DIALECT_FLAGS)
        .args(["-w", "-g", "-O0", "-c"])
        .args(flags)
        .arg(source)
        .arg("-o")
        .arg(out)
        .output()
        .map_err(|e| format!("cannot run `{}`: {e}", tools.cc))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

/// Compile preprocessed C `text`, saved as `file`.
pub(crate) fn compile_text(
    tools: &Toolchain,
    file: &Path,
    text: &str,
    out: &Path,
    extra: &[&str],
) -> Result<(), String> {
    std::fs::write(file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    let extra: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
    run_cc(tools, &extra, file, out)
}

/// `source` preprocessed with `flags`, line markers removed as the server removed them, so a
/// chunk's text is found verbatim.
fn preprocess(tools: &Toolchain, flags: &[String], source: &Path) -> Result<String, String> {
    let output = Command::new(&tools.cc)
        .arg("-E")
        .args(DIALECT_FLAGS)
        .args(flags)
        .arg(source)
        .output()
        .map_err(|e| format!("cannot run `{}`: {e}", tools.cc))?;
    if !output.status.success() {
        return Err(first_error(&String::from_utf8_lossy(&output.stderr)));
    }
    let mapped = Mapped::new(&output.stdout);
    Ok(String::from_utf8_lossy(&mapped.text).into_owned())
}

/// Link `objects` (and `rust`) into `dir/program`: what failed, and the linker's output.
pub(crate) fn link(
    tools: &Toolchain,
    dir: &Path,
    objects: &[PathBuf],
    rust: Option<&Path>,
) -> (Problems, String) {
    let mut cmd = Command::new(&tools.cc);
    cmd.arg("-o").arg(dir.join("program")).args(objects);
    if let Some(lib) = rust {
        cmd.arg(lib);
    }
    cmd.args(&tools.libs);
    match cmd.output() {
        Ok(out) => {
            let mut log = String::from_utf8_lossy(&out.stderr).into_owned();
            let mut found = problems(&log, !out.status.success());
            // macOS's ld stops at undefined symbols before it reports duplicates (GNU ld names
            // both): link again letting undefined ones be, to find any duplicates too.
            if cfg!(target_os = "macos") && !found.undefined.is_empty() {
                let mut again = Command::new(&tools.cc);
                again.arg("-o").arg(dir.join("program")).args(objects);
                if let Some(lib) = rust {
                    again.arg(lib);
                }
                again.args(&tools.libs).arg("-Wl,-undefined,dynamic_lookup");
                if let Ok(out) = again.output() {
                    let more = String::from_utf8_lossy(&out.stderr).into_owned();
                    let dup = problems(&more, !out.status.success()).duplicate;
                    if !dup.is_empty() {
                        found.duplicate.extend(dup);
                        log.push_str(&more);
                    }
                }
            }
            (found, log)
        }
        Err(e) => {
            let log = format!("cannot run `{}`: {e}", tools.cc);
            let mut p = Problems {
                failed: true,
                ..Problems::default()
            };
            p.other.insert(log.clone());
            (p, log)
        }
    }
}

static UNDEFINED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"undefined reference to [`'‘"]([^'’"`]+)['’"`]|undefined symbol: (\S+)"#).unwrap()
});
static DUPLICATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"multiple definition of [`'‘"]([^'’"`]+)['’"`]|duplicate symbol: (\S+)"#).unwrap()
});
/// macOS's ld: `  "_add", referenced from:` under `Undefined symbols for architecture …:`, and
/// `duplicate symbol '_helper' in:`; both name Mach-O symbols (a `_` before the C name).
static MACOS_UNDEFINED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s+"_([^"]+)", referenced from:"#).unwrap());
static MACOS_DUPLICATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"duplicate symbol '_([^']+)'"#).unwrap());

/// GNU ld's, lld's and mold's complaints, by symbol.
pub fn problems(log: &str, failed: bool) -> Problems {
    let mut p = Problems {
        failed,
        ..Problems::default()
    };
    let symbol = |c: regex::Captures<'_>| {
        c.get(1)
            .or_else(|| c.get(2))
            .map(|m| m.as_str().to_string())
    };
    for line in log.lines() {
        if let Some(s) = UNDEFINED.captures(line).and_then(symbol) {
            p.undefined.insert(s);
        } else if let Some(s) = DUPLICATE.captures(line).and_then(symbol) {
            p.duplicate.insert(s);
        } else if let Some(s) = MACOS_UNDEFINED.captures(line).and_then(symbol) {
            p.undefined.insert(s);
        } else if let Some(s) = MACOS_DUPLICATE.captures(line).and_then(symbol) {
            p.duplicate.insert(s);
        } else if line.contains("error")
            && !line.contains("ld returned")
            && !line.contains("linker command failed")
        {
            p.other.insert(line.trim().to_string());
        }
    }
    p
}

/// Defined and undefined symbols of an object file, by binding. Section and file symbols,
/// assembler-local labels and Rust's own internals are left out.
pub fn symbols(bytes: &[u8]) -> Result<Symbols, String> {
    let file = object::File::parse(bytes).map_err(|e| e.to_string())?;
    // Mach-O (macOS) spells every C symbol with a leading `_`: read it as the C name.
    let macho = file.format() == BinaryFormat::MachO;
    let mut out = Symbols::default();
    for sym in file.symbols() {
        if matches!(sym.kind(), SymbolKind::Section | SymbolKind::File) {
            continue;
        }
        let Ok(name) = sym.name() else { continue };
        let name = if macho { name.strip_prefix('_').unwrap_or(name) } else { name };
        if name.is_empty() || name.starts_with('.') || internal(name) {
            continue;
        }
        let set = if sym.is_undefined() {
            &mut out.undefined
        } else if sym.is_global() || sym.is_weak() {
            &mut out.global
        } else {
            &mut out.local
        };
        set.insert(name.to_string());
    }
    Ok(out)
}

/// Symbols of Rust's runtime and of the compiler, never a C name a candidate chose.
pub fn internal(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "_ZN",
        "_R",
        "__rust",
        "rust_",
        "__rdl",
        "__rg_",
        "anon.",
        "GCC_except_table",
        "DW.ref.",
        "__gcc_personality",
        "_GLOBAL_OFFSET_TABLE_",
        "_Unwind_",
        "__cxa_",
    ];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

/// The first error line of a compiler's output.
pub(crate) fn first_error(log: &str) -> String {
    log.lines()
        .find(|l| l.contains("error"))
        .or_else(|| log.lines().next())
        .unwrap_or("no output")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests;
