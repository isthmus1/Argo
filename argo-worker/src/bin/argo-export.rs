//! `argo-export`: write a database's translated Rust out as a standalone crate.
//!
//! One module per original C file, holding every function whose Rust was pushed (`Done`), each
//! under a comment with its original C and where it came from; `REPORT.md` lists what was
//! exported and what wasn't (not translated yet, failed, or a symbol another file already
//! exports). The crate is its own workspace and builds as a static library, the form it would be
//! linked into the C program in.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use argo_db_client::{CChunk, ChunkKind, RustChunk, RustStatus};
use clap::Parser;

use argo_worker::db::Db;

/// Export the translated Rust of a database as a crate.
#[derive(Debug, Parser)]
struct Args {
    /// SpacetimeDB host, e.g. `ws://127.0.0.1:3100`.
    #[arg(long)]
    db: String,
    /// Database name the module is published under.
    #[arg(long, default_value = "argo")]
    module: String,
    /// Directory to write the crate to (created; files in it are replaced).
    #[arg(long)]
    out: PathBuf,
    /// The crate's name.
    #[arg(long, default_value = "argo_rewrite")]
    name: String,
    /// Also write the C program with every accepted piece spliced in to `<out>/mixed`, from this
    /// clone of the repository at the ingested commit (a worker's clone will do). Build it with
    /// the project's own build, linking `<out>`'s static library.
    #[arg(long, value_name = "CHECKOUT")]
    mixed: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args).await {
        Ok(summary) => {
            eprintln!("argo-export: {summary}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("argo-export: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: &Args) -> Result<String, String> {
    let db = Db::connect(&args.db, &args.module).await?;
    let (chunks, rusts) = (db.chunks(), db.rusts());
    db.close();
    let ingested = db.ingested()?;
    let origin = ingested.as_ref().map_or_else(|| args.module.clone(), |o| o.locator.clone());
    let crate_ = build(&chunks, &rusts, &args.name, &origin);
    write(&args.out, &crate_)?;
    let mut summary = format!(
        "{} functions exported to {} ({} modules); {} not exported, see REPORT.md",
        crate_.exported,
        args.out.display(),
        crate_.modules.len(),
        crate_.left_out
    );
    if let Some(checkout) = &args.mixed {
        let units: Vec<String> = ingested
            .iter()
            .flat_map(|o| &o.units)
            .filter(|u| u.skipped.is_none())
            .map(|u| u.path.clone())
            .collect();
        let out = args.out.join("mixed");
        let m = argo_worker::mixed::write(&chunks, &rusts, &units, checkout, &out)?;
        for (id, why) in &m.skipped {
            eprintln!("argo-export: chunk {id} stays C in the mixed program: {why}");
        }
        let _ = write!(
            summary,
            "; mixed C program in {}: {} pieces and {} functions spliced into {} files, {} left as C",
            out.display(),
            m.pieces,
            m.functions,
            m.files,
            m.skipped.len()
        );
    }
    Ok(summary)
}

struct Crate {
    files: Vec<(PathBuf, String)>,
    modules: Vec<String>,
    exported: usize,
    left_out: usize,
}

/// The crate's files, from the database's rows.
fn build(chunks: &[CChunk], rusts: &[RustChunk], name: &str, origin: &str) -> Crate {
    let rust_of: BTreeMap<u64, &RustChunk> = rusts.iter().map(|r| (r.chunk, r)).collect();
    let mut functions: Vec<&CChunk> = chunks.iter().filter(|c| c.translatable).collect();
    functions.sort_by(|a, b| (&a.path, a.line_start, a.id).cmp(&(&b.path, b.line_start, b.id)));

    let mut by_module: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut symbols = HashSet::new();
    let (mut exported, mut left_out) = (0, 0);
    let mut report = String::new();
    let mut rows = String::new();
    for c in &functions {
        let path = if c.path.is_empty() { "unknown".to_string() } else { c.path.clone() };
        let (status, why) = match rust_of.get(&c.id) {
            None => ("left out", "not translated yet".to_string()),
            Some(r) if r.status == RustStatus::Failed => ("left out", "translation failed".to_string()),
            Some(r) if r.status == RustStatus::Claimed => ("left out", "still being translated".to_string()),
            // A piece's name is its C text, which repeats; its symbol is unique per row.
            Some(_) if c.kind == ChunkKind::Function && !symbols.insert(c.name.clone()) => {
                ("left out", "another file already exports this symbol".to_string())
            }
            Some(r) => {
                let module = by_module.entry(module_name(&path)).or_insert_with(|| (path.clone(), String::new()));
                let _ = writeln!(
                    module.1,
                    "// ── `{}` · {}:{}-{} · chunk {} · {} · {} attempt(s) · {:?}\n//\n// Original C:\n{}\n\
                     // Its own module: each chunk's file brings its own `use`s and C type stubs.\n\
                     pub mod chunk{} {{\n{}\n}}\n",
                    c.name,
                    path,
                    c.line_start,
                    c.line_end,
                    c.id,
                    r.model,
                    r.attempts,
                    r.validation,
                    commented(&c.source),
                    c.id,
                    r.source.trim_end()
                );
                exported += 1;
                ("exported", format!("{:?}", r.validation))
            }
        };
        if status != "exported" {
            left_out += 1;
        }
        let _ = writeln!(rows, "| `{}` | {} | {} | {} |", c.name, path, status, why);
    }

    let mut lib = format!(
        "//! Rust rewritten from {origin} by Argo: one module per original C file.\n\
         //! Generated by `argo-export`; see REPORT.md.\n\n\
         #![allow(unused, non_snake_case, non_camel_case_types, non_upper_case_globals, clippy::all)]\n\n"
    );
    let mut files = Vec::new();
    for (module, (path, body)) in &by_module {
        let _ = writeln!(lib, "/// From `{path}`.\npub mod {module};");
        files.push((PathBuf::from(format!("src/{module}.rs")), format!("//! From `{path}`.\n\n{body}")));
    }
    files.push((PathBuf::from("src/lib.rs"), lib));
    files.push((
        PathBuf::from("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n\
             [lib]\ncrate-type = [\"staticlib\", \"rlib\"]\n\n\
             # Its own workspace, so it builds wherever it is written.\n[workspace]\n"
        ),
    ));
    let _ = writeln!(
        report,
        "# Argo rewrite of {origin}\n\n\
         {exported} of {} functions exported into {} modules; {left_out} left out.\n\n\
         Every exported function compiled on its own with rustc when its worker checked it. \
         `cargo build` here checks that they also compile together as one crate.\n\n\
         | Function | File | Status | Detail |\n|---|---|---|---|\n{rows}",
        functions.len(),
        by_module.len(),
    );
    files.push((PathBuf::from("REPORT.md"), report));
    Crate { files, modules: by_module.into_keys().collect(), exported, left_out }
}

/// A Rust module name for an original file: `slirp/ip_input.c` → `slirp_ip_input_c`.
fn module_name(path: &str) -> String {
    let mut m: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    if m.starts_with(|c: char| c.is_ascii_digit()) {
        m.insert(0, '_');
    }
    m
}

/// C as `//` comment lines.
fn commented(c: &str) -> String {
    c.lines().map(|l| format!("//     {l}")).collect::<Vec<_>>().join("\n")
}

fn write(out: &Path, c: &Crate) -> Result<(), String> {
    // Replace the generated sources, so a function exported last time but not now is gone.
    let _ = std::fs::remove_dir_all(out.join("src"));
    for (path, text) in &c.files {
        let path = out.join(path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_names_are_rust_identifiers() {
        assert_eq!(module_name("slirp/ip_input.c"), "slirp_ip_input_c");
        assert_eq!(module_name("fs-utils.h"), "fs_utils_h");
        assert_eq!(module_name("9p.c"), "_9p_c");
    }
}
