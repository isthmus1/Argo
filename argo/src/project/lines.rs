//! `cc -E` line markers turned back into original locations.
//!
//! The preprocessor's output keeps `# <line> "<file>" <flags>` markers; they are removed from the
//! text the parser reads and kept as a per-line map instead.

use std::path::PathBuf;

/// Preprocessed text without directives, and where each of its lines came from.
pub struct Mapped {
    pub text: Vec<u8>,
    /// Byte offset of each line in `text`.
    line_starts: Vec<u32>,
    /// Per line: index into `files` and the original 1-based line.
    lines: Vec<(u32, u32)>,
    /// Original file names exactly as the preprocessor spelled them.
    pub files: Vec<String>,
}

pub struct Origin<'a> {
    pub file: &'a str,
    pub line: u32,
}

impl Mapped {
    /// Strip line markers and other directives (`#pragma`) from `cc -E` output.
    pub fn new(raw: &[u8]) -> Mapped {
        let mut m = Mapped {
            text: Vec::with_capacity(raw.len()),
            line_starts: vec![],
            lines: vec![],
            files: vec![],
        };
        // Include stack of file indices; the last is the file being read.
        let mut stack: Vec<u32> = vec![];
        let mut line = 1u32;
        let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
        for raw_line in raw.split(|&b| b == b'\n') {
            let trimmed = raw_line.trim_ascii_start();
            if trimmed.first() == Some(&b'#') {
                if let Some((n, file, flags)) = marker(trimmed) {
                    let f = m.file_index(&file);
                    if flags.contains(&1) {
                        stack.push(f);
                    } else if flags.contains(&2) {
                        while stack.len() > 1 && stack.last() != Some(&f) {
                            stack.pop();
                        }
                        if stack.last() != Some(&f) {
                            stack.push(f);
                        }
                    } else if let Some(top) = stack.last_mut() {
                        *top = f;
                    } else {
                        stack.push(f);
                    }
                    line = n;
                }
                continue;
            }
            m.line_starts.push(m.text.len() as u32);
            m.text.extend_from_slice(raw_line);
            m.text.push(b'\n');
            m.lines.push((stack.last().copied().unwrap_or(0), line));
            line += 1;
        }
        if m.files.is_empty() {
            m.files.push("<input>".into());
        }
        m
    }

    fn file_index(&mut self, name: &str) -> u32 {
        match self.files.iter().position(|f| f == name) {
            Some(i) => i as u32,
            None => {
                self.files.push(name.to_string());
                self.files.len() as u32 - 1
            }
        }
    }

    /// Where the byte at `pos` of `text` came from.
    pub fn origin(&self, pos: u32) -> Origin<'_> {
        let i = self
            .line_starts
            .partition_point(|&s| s <= pos)
            .saturating_sub(1);
        let (file, line) = self.lines.get(i).copied().unwrap_or((0, 1));
        Origin {
            file: &self.files[file as usize],
            line,
        }
    }

    /// The lines overlapping bytes `lo..hi` of `text`, in order: where each starts (`lo` for
    /// the first when it starts earlier), and its file (index into `files`) and original line.
    pub fn lines_in(&self, lo: u32, hi: u32) -> impl Iterator<Item = (u32, u32, u32)> + '_ {
        let first = self
            .line_starts
            .partition_point(|&s| s <= lo)
            .saturating_sub(1);
        self.line_starts[first..]
            .iter()
            .zip(&self.lines[first..])
            .take_while(move |(s, _)| **s < hi.max(lo + 1))
            .map(move |(&s, &(file, line))| (s.max(lo), file, line))
    }
}

/// `# 12 "path" 1 3` → (12, path, [1, 3]).
fn marker(line: &[u8]) -> Option<(u32, String, Vec<u8>)> {
    let s = std::str::from_utf8(line).ok()?;
    let rest = s.strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix("line").map_or(rest, str::trim_start);
    let (num, rest) = rest.split_once(' ')?;
    let n: u32 = num.parse().ok()?;
    let rest = rest.trim_start().strip_prefix('"')?;
    // File names escape `"` and `\` with a backslash.
    let mut name = String::new();
    let mut chars = rest.chars();
    loop {
        match chars.next()? {
            '\\' => name.push(chars.next()?),
            '"' => break,
            c => name.push(c),
        }
    }
    let flags = chars
        .as_str()
        .split_whitespace()
        .filter_map(|f| f.parse().ok())
        .collect();
    Some((n, name, flags))
}

/// Canonical form of a preprocessor file name, for comparing against the project root.
pub fn canonical(name: &str) -> Option<PathBuf> {
    if name.starts_with('<') {
        return None;
    }
    std::fs::canonicalize(name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_become_a_line_map() {
        let raw = b"# 1 \"<stdin>\"\n# 1 \"a.c\" 1\nint a;\n# 1 \"a.h\" 1\nint h;\n# 3 \"a.c\" 2\n#pragma once\nint b;\n# 2 \"<stdin>\" 2\n# 1 \"b.c\" 1\nint c;\n";
        let m = Mapped::new(raw);
        assert_eq!(
            String::from_utf8_lossy(&m.text),
            "int a;\nint h;\nint b;\nint c;\n"
        );
        let at = |s: &str| {
            let pos = m
                .text
                .windows(s.len())
                .position(|w| w == s.as_bytes())
                .unwrap() as u32;
            let o = m.origin(pos);
            (o.file.to_string(), o.line)
        };
        assert_eq!(at("int a"), ("a.c".into(), 1));
        assert_eq!(at("int h"), ("a.h".into(), 1));
        assert_eq!(at("int b"), ("a.c".into(), 3));
        assert_eq!(at("int c"), ("b.c".into(), 1));
        // Bytes 9..20 end line 2 and cover line 3; the first is clamped to where the range starts.
        let name = |f: u32| m.files[f as usize].as_str();
        let lines: Vec<_> = m.lines_in(9, 20).map(|(s, f, l)| (s, name(f), l)).collect();
        assert_eq!(lines, [(9, "a.h", 1), (14, "a.c", 3)]);
    }

    #[test]
    fn escaped_file_names() {
        assert_eq!(
            marker(br#"# 7 "dir/we\"ird.h" 1 3 4"#),
            Some((7, "dir/we\"ird.h".into(), vec![1, 3, 4]))
        );
        assert_eq!(marker(b"#pragma pack(1)"), None);
    }
}
