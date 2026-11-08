//! The agent's read-only tools: `read`, `grep` and `glob`, over the repository clone and the
//! claim's session directory (the ingested C program and each submission's source and logs).
//!
//! There is deliberately no tool that writes or runs anything: the clone is shared by every
//! reader of this worker and must stay exactly the ingested commit, and `submit_rewrite` is the
//! only way to build code. Every path is resolved through the filesystem (`canonicalize`), so
//! neither `..` nor a symlink reaches outside the two roots, and every output is bounded.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde_json::{Value, json};

use crate::model::ToolSpec;

/// Lines `read` returns when no limit is given, and at most.
const READ_LINES: usize = 400;
const MAX_READ_LINES: usize = 2000;
/// Any one line is cut after this many characters.
const LINE_CHARS: usize = 400;
/// Every tool's output is cut after this many bytes.
const OUTPUT_BYTES: usize = 48 << 10;
const GREP_MATCHES: usize = 200;
const GLOB_PATHS: usize = 500;
/// Files larger than this are not searched.
const SEARCH_FILE_BYTES: u64 = 4 << 20;

/// Where the tools may look.
#[derive(Debug, Clone)]
pub struct Tools {
    /// The clone at the ingested commit, canonical; `None` when the ingest has no commit.
    repo: Option<PathBuf>,
    /// The claim's session directory, canonical.
    session: PathBuf,
}

impl Tools {
    pub fn new(repo: Option<&Path>, session: &Path) -> Result<Tools, String> {
        let canonical = |p: &Path| {
            p.canonicalize()
                .map_err(|e| format!("{}: {e}", p.display()))
        };
        Ok(Tools {
            repo: repo.map(canonical).transpose()?,
            session: canonical(session)?,
        })
    }

    pub fn specs() -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "read",
                description: "Read a text file with line numbers. `path` is relative to the \
                    repository root, or an absolute path inside the session directory (the \
                    ingested C program and your submissions' sources and logs). Returns at most \
                    2000 lines per call; use `offset` and `limit` to page.",
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File to read."},
                        "offset": {"type": "integer", "minimum": 1, "description": "First line to return (1-based). Default 1."},
                        "limit": {"type": "integer", "minimum": 1, "description": "Number of lines to return. Default 400, at most 2000."}
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }),
            },
            ToolSpec {
                name: "grep",
                description: "Search file contents with a regular expression (Rust regex \
                    syntax). Returns matching lines as `path:line: text`, at most 200. Searches \
                    the repository unless `path` names a file or directory to search instead.",
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Regular expression, e.g. `struct\\s+VirtMachine\\b`."},
                        "path": {"type": "string", "description": "File or directory to search. Default: the repository root."},
                        "glob": {"type": "string", "description": "Only search files matching this glob, e.g. `*.h` or `src/**/*.c`."},
                        "ignore_case": {"type": "boolean", "description": "Case-insensitive search. Default false."}
                    },
                    "required": ["pattern"],
                    "additionalProperties": false
                }),
            },
            ToolSpec {
                name: "glob",
                description: "List files whose path matches a glob pattern such as `**/*.h` or \
                    `src/*.c`, relative to `path` (default: the repository root). At most 500.",
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Glob pattern."},
                        "path": {"type": "string", "description": "Directory to match under. Default: the repository root."}
                    },
                    "required": ["pattern"],
                    "additionalProperties": false
                }),
            },
        ]
    }

    /// Whether `name` is one of these tools.
    pub fn has(name: &str) -> bool {
        matches!(name, "read" | "grep" | "glob")
    }

    /// Run tool `name` on `args`; the text goes back to the model, errors included.
    pub fn run(&self, name: &str, args: &Value) -> String {
        let result = match name {
            "read" => self.read(args),
            "grep" => self.grep(args),
            "glob" => self.glob(args),
            _ => Err(format!("unknown tool `{name}`")),
        };
        let text = result.unwrap_or_else(|e| format!("error: {e}"));
        cut(text, OUTPUT_BYTES)
    }

    /// `path` inside one of the roots, or why not. Relative paths are the repository's.
    fn resolve(&self, path: Option<&str>) -> Result<PathBuf, String> {
        let path = path.unwrap_or("").trim();
        let candidate = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            let repo = self.repo.as_ref().ok_or(
                "there is no repository checkout for this chunk; only absolute paths in the \
                 session directory can be read",
            )?;
            repo.join(path)
        };
        let real = candidate
            .canonicalize()
            .map_err(|e| format!("{path}: {e}"))?;
        if self
            .repo
            .iter()
            .chain([&self.session])
            .any(|root| real.starts_with(root))
        {
            Ok(real)
        } else {
            Err(format!(
                "{path} is outside the repository and the session directory"
            ))
        }
    }

    /// How a resolved path is shown: relative to the repository, else absolute.
    fn show(&self, path: &Path) -> String {
        match self.repo.as_ref().and_then(|r| path.strip_prefix(r).ok()) {
            Some(rel) if rel.as_os_str().is_empty() => ".".into(),
            Some(rel) => rel.display().to_string(),
            None => path.display().to_string(),
        }
    }

    fn read(&self, args: &Value) -> Result<String, String> {
        let path = self.resolve(Some(string(args, "path")?))?;
        if path.is_dir() {
            return Err(format!(
                "{} is a directory; use glob to list it",
                self.show(&path)
            ));
        }
        let offset = number(args, "offset")?.unwrap_or(1).max(1);
        let limit = number(args, "limit")?
            .unwrap_or(READ_LINES)
            .clamp(1, MAX_READ_LINES);
        let file = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", self.show(&path)))?;
        let mut out = String::new();
        let (mut total, mut last) = (0, 0);
        for (i, line) in BufReader::new(file).split(b'\n').enumerate() {
            let line = line.map_err(|e| e.to_string())?;
            total = i + 1;
            if total >= offset && total < offset + limit {
                let text = String::from_utf8_lossy(&line);
                let _ = writeln!(out, "{total:>6}\t{}", cut_line(text.trim_end_matches('\r')));
                last = total;
            }
        }
        if total == 0 {
            return Ok(format!("{} is empty", self.show(&path)));
        }
        if last == 0 {
            return Err(format!(
                "{} has {total} lines; offset {offset} is past the end",
                self.show(&path)
            ));
        }
        if last < total {
            let _ = write!(
                out,
                "[lines {offset}-{last} of {total}; read with offset={} for more]",
                last + 1
            );
        }
        Ok(out)
    }

    fn grep(&self, args: &Value) -> Result<String, String> {
        let pattern = string(args, "pattern")?;
        let ignore_case = args
            .get("ignore_case")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(ignore_case)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| format!("invalid pattern: {e}"))?;
        let base = self.resolve(optional(args, "path")?)?;
        let filter = optional(args, "glob")?.map(matcher).transpose()?;
        let mut out = String::new();
        let mut matches = 0;
        for file in self.files(&base) {
            if let Some(f) = &filter
                && !matches_glob(f, &base, &file)
            {
                continue;
            }
            let Some(text) = text_of(&file) else { continue };
            for (n, line) in text.lines().enumerate() {
                if !regex.is_match(line) {
                    continue;
                }
                matches += 1;
                if matches > GREP_MATCHES {
                    let _ = write!(out, "[more than {GREP_MATCHES} matches; narrow the search]");
                    return Ok(out);
                }
                let _ = writeln!(out, "{}:{}: {}", self.show(&file), n + 1, cut_line(line));
            }
        }
        if matches == 0 {
            return Ok("no matches".into());
        }
        Ok(out)
    }

    fn glob(&self, args: &Value) -> Result<String, String> {
        let pattern = matcher(string(args, "pattern")?)?;
        let base = self.resolve(optional(args, "path")?)?;
        if !base.is_dir() {
            return Err(format!("{} is not a directory", self.show(&base)));
        }
        let mut found: Vec<String> = self
            .files(&base)
            .filter(|f| matches_glob(&pattern, &base, f))
            .map(|f| self.show(&f))
            .collect();
        found.sort();
        if found.is_empty() {
            return Ok("no files match".into());
        }
        let more = found.len().saturating_sub(GLOB_PATHS);
        found.truncate(GLOB_PATHS);
        let mut out = found.join("\n");
        if more > 0 {
            let _ = write!(out, "\n[{more} more; narrow the pattern]");
        }
        Ok(out)
    }

    /// Files under `base` (or `base` itself), skipping `.git` and what `.gitignore` ignores.
    /// Symlinks are not followed, so a walk never leaves its root.
    fn files(&self, base: &Path) -> impl Iterator<Item = PathBuf> {
        WalkBuilder::new(base)
            .hidden(false)
            .follow_links(false)
            .require_git(false)
            .filter_entry(|e| e.file_name() != ".git")
            .build()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .map(ignore::DirEntry::into_path)
    }
}

fn matcher(pattern: &str) -> Result<GlobMatcher, String> {
    Glob::new(pattern)
        .map(|g| g.compile_matcher())
        .map_err(|e| format!("invalid glob: {e}"))
}

/// A pattern with no `/` matches a file name anywhere (`*.h`); otherwise the path below `base`.
fn matches_glob(glob: &GlobMatcher, base: &Path, file: &Path) -> bool {
    let rel = file.strip_prefix(base).unwrap_or(file);
    if glob.glob().glob().contains('/') {
        glob.is_match(rel)
    } else {
        file.file_name().is_some_and(|n| glob.is_match(n))
    }
}

/// A searchable file's text: not too large, and not binary.
fn text_of(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > SEARCH_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(SEARCH_FILE_BYTES).read_to_end(&mut bytes).ok()?;
    if bytes.iter().take(8 << 10).any(|&b| b == 0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    optional(args, key)?.ok_or_else(|| format!("missing string argument `{key}`"))
}

fn optional<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => Err(format!("`{key}` must be a string, not {other}")),
    }
}

fn number(args: &Value, key: &str) -> Result<Option<usize>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .map(|n| Some(n as usize))
            .ok_or_else(|| format!("`{key}` must be a positive integer, not {v}")),
    }
}

fn cut_line(line: &str) -> String {
    match line.char_indices().nth(LINE_CHARS) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_string(),
    }
}

/// `text` cut to `max` bytes (whole characters), saying so.
pub fn cut(mut text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let mut at = max;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    text.truncate(at);
    text.push_str("\n[output cut; narrow the request]");
    text
}

#[cfg(test)]
mod tests;
