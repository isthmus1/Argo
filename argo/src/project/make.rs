//! Preprocessor flags read out of a project's Makefile without running make or a shell.
//!
//! A small evaluator: variable assignments (`=`, `:=`, `+=`, `?=`), `$(VAR)` expansion,
//! `ifdef`/`ifndef`/`ifeq`/`ifneq`/`else`/`endif`, and `include` of other files in the project.
//! `$(shell cat FILE)` reads FILE (inside the project); every other function expands to nothing,
//! so nothing from the Makefile is ever executed. From the result it takes the `-D`/`-U`/`-I`/
//! `-include` flags of `CFLAGS`-like variables, and the extra flags of explicit rules that compile
//! one `.c` file (`riscv_cpu64.o: riscv_cpu.c` with `$(CC) $(CFLAGS) -DMAX_XLEN=64 ...`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Stands in for text make would compute but this evaluator can't (`$(shell git describe)`).
const UNKNOWN: char = '\u{1}';
/// Variables whose flags apply to every file.
const FLAG_VARS: [&str; 7] = [
    "CFLAGS", "CPPFLAGS", "DEFINES", "DEFS", "INCLUDES", "INCS", "INC",
];

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Makefile {
    /// Flags for every file, in Makefile order.
    pub flags: Vec<String>,
    /// The `-D` flags among `flags` that were added inside a conditional (`ifdef CONFIG_SDL`):
    /// optional features, which a file may be read without when they need a missing header.
    pub optional: Vec<String>,
    /// Extra flags for single files, from explicit rules; the last rule for a file wins.
    pub per_file: Vec<(PathBuf, Vec<String>)>,
}

impl Makefile {
    pub fn flags_for(&self, file: &Path) -> &[String] {
        self.per_file
            .iter()
            .find(|(p, _)| p == file)
            .map_or(&[], |(_, f)| f.as_slice())
    }
}

/// Read `root`'s top-level Makefile (GNUmakefile, makefile or Makefile), if there is one.
pub fn read(root: &Path) -> Makefile {
    let Some(name) = ["GNUmakefile", "makefile", "Makefile"]
        .iter()
        .find(|f| root.join(f).is_file())
    else {
        return Makefile::default();
    };
    let mut ev = Eval {
        root,
        vars: HashMap::new(),
        conds: vec![],
        rule: None,
        optional: vec![],
        rules: vec![],
        rule_id: 0,
        in_rule: false,
    };
    ev.file(&root.join(name), 0);
    let mut flags = vec![];
    for var in FLAG_VARS {
        if let Some(v) = ev.vars.get(var).cloned() {
            for f in compiler_flags(&ev.expand(&v, 0), root) {
                if !flags.contains(&f) {
                    flags.push(f);
                }
            }
        }
    }
    let optional = ev
        .optional
        .iter()
        .filter(|f| flags.contains(f))
        .cloned()
        .fold(vec![], |mut acc, f| {
            if !acc.contains(&f) {
                acc.push(f);
            }
            acc
        });
    let mut per_file: Vec<(PathBuf, Vec<String>)> = vec![];
    // Recipe lines of one rule add up; a later rule for the same file replaces an earlier one.
    let mut last: Option<(PathBuf, usize)> = None;
    for (file, recipe, id) in ev.rules {
        let extra: Vec<String> = compiler_flags(&recipe, root)
            .into_iter()
            .filter(|f| !flags.contains(f))
            .collect();
        let same_rule = last.as_ref() == Some(&(file.clone(), id));
        if !same_rule {
            per_file.retain(|(p, _)| *p != file);
            per_file.push((file.clone(), vec![]));
        }
        if let Some((_, fs)) = per_file.iter_mut().find(|(p, _)| *p == file) {
            for f in extra {
                if !fs.contains(&f) {
                    fs.push(f);
                }
            }
        }
        last = Some((file, id));
    }
    per_file.retain(|(_, fs)| !fs.is_empty());
    Makefile {
        flags,
        optional,
        per_file,
    }
}

struct Cond {
    /// This branch is being read.
    on: bool,
    /// Some branch of this conditional has been taken already.
    taken: bool,
    /// The enclosing conditionals are all on.
    parent: bool,
}

struct Eval<'a> {
    root: &'a Path,
    /// Raw values; `:=` stores the expanded value.
    vars: HashMap<String, String>,
    conds: Vec<Cond>,
    /// The `.c` file the rule being read compiles, if it compiles exactly one.
    rule: Option<PathBuf>,
    optional: Vec<String>,
    /// (source file, one expanded recipe line, the rule it belongs to).
    rules: Vec<(PathBuf, String, usize)>,
    /// Rules seen so far (an id for the rule being read).
    rule_id: usize,
    /// Tab-indented lines are recipes: true after a rule line, until an assignment or include.
    in_rule: bool,
}

impl Eval<'_> {
    fn active(&self) -> bool {
        self.conds.last().is_none_or(|c| c.on && c.parent)
    }

    fn file(&mut self, path: &Path, depth: usize) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        for line in logical_lines(&text) {
            self.line(&line, depth);
        }
    }

    fn line(&mut self, raw: &str, depth: usize) {
        // A tab starts a recipe line only inside a rule; make still reads conditionals there, and
        // outside a rule a tab-indented line (`\tCFLAGS += …` under `ifeq`) is ordinary text.
        if let Some(recipe) = raw.strip_prefix('\t')
            && self.in_rule
            && !self.conditional(strip_comment(recipe).trim())
        {
            if self.active()
                && let Some(file) = self.rule.clone()
            {
                let expanded = self.expand(recipe, 0);
                self.rules.push((file, expanded, self.rule_id));
            }
            return;
        } else if raw.starts_with('\t') && self.in_rule {
            return; // a conditional inside a recipe, already handled
        }
        let line = strip_comment(raw);
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        if self.conditional(line) {
            return;
        }
        if !self.active() {
            return;
        }
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        if matches!(word, "include" | "-include" | "sinclude") {
            self.in_rule = false;
            if depth < 8 {
                for f in self.expand(rest, 0).split_whitespace() {
                    if f.contains(UNKNOWN) {
                        continue;
                    }
                    let p = self.root.join(f);
                    if let Ok(p) = std::fs::canonicalize(p)
                        && p.starts_with(self.root)
                    {
                        self.file(&p, depth + 1);
                    }
                }
            }
            return;
        }
        if let Some((name, op, value)) = assignment(line) {
            self.rule = None;
            self.in_rule = false;
            self.assign(name, op, value);
            return;
        }
        // A rule: `targets: prerequisites [; recipe]`.
        self.rule = None;
        if let Some((targets, prereqs)) = line.split_once(':') {
            self.in_rule = true;
            self.rule_id += 1;
            let prereqs = prereqs.split(';').next().unwrap_or("");
            let targets = self.expand(targets, 0);
            let prereqs = self.expand(prereqs, 0);
            let one_target = targets.split_whitespace().count() == 1;
            let sources: Vec<&str> = prereqs
                .split_whitespace()
                .filter(|p| p.ends_with(".c") && !p.contains('%'))
                .collect();
            if one_target
                && let [src] = sources[..]
                && let Ok(p) = std::fs::canonicalize(self.root.join(src))
            {
                self.rule = Some(p);
            }
        }
    }

    /// Handle a conditional directive; false when `line` isn't one.
    fn conditional(&mut self, line: &str) -> bool {
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match word {
            "ifdef" | "ifndef" | "ifeq" | "ifneq" => {
                let parent = self.active();
                let on = parent && self.test(word, rest);
                self.conds.push(Cond {
                    on,
                    taken: on,
                    parent,
                });
                true
            }
            "else" => {
                let parent_on = self.conds.last().is_some_and(|c| c.parent);
                let test = if rest.is_empty() {
                    None
                } else {
                    let (w, r) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                    Some((w.to_string(), r.trim().to_string()))
                };
                let passes = match &test {
                    None => true,
                    Some((w, r)) => parent_on && self.test(w, r),
                };
                if let Some(c) = self.conds.last_mut() {
                    c.on = !c.taken && passes;
                    c.taken |= c.on;
                }
                true
            }
            "endif" => {
                self.conds.pop();
                true
            }
            _ => false,
        }
    }

    fn test(&mut self, word: &str, rest: &str) -> bool {
        match word {
            "ifdef" | "ifndef" => {
                let name = self.expand(rest, 0);
                let defined = self
                    .vars
                    .get(name.trim())
                    .is_some_and(|v| !v.trim().is_empty());
                defined == (word == "ifdef")
            }
            "ifeq" | "ifneq" => {
                let Some((a, b)) = comparison(rest) else {
                    return false;
                };
                let (a, b) = (self.expand(&a, 0), self.expand(&b, 0));
                if a.contains(UNKNOWN) || b.contains(UNKNOWN) {
                    // Can't tell (`ifeq ($(shell uname),Darwin)`): the test fails either way,
                    // so its `else` branch is read.
                    return false;
                }
                (a.trim() == b.trim()) == (word == "ifeq")
            }
            _ => false,
        }
    }

    fn assign(&mut self, name: &str, op: &str, value: &str) {
        let value = value.trim();
        let new = match op {
            ":=" | "::=" | ":::=" => self.expand(value, 0),
            "+=" => {
                let old = self.vars.get(name).cloned().unwrap_or_default();
                if old.is_empty() {
                    value.to_string()
                } else {
                    format!("{old} {value}")
                }
            }
            "?=" if self.vars.contains_key(name) => return,
            _ => value.to_string(),
        };
        if !self.conds.is_empty() && FLAG_VARS.contains(&name) {
            let expanded = self.expand(value, 0);
            self.optional.extend(
                compiler_flags(&expanded, self.root)
                    .into_iter()
                    .filter(|f| f.starts_with("-D")),
            );
        }
        self.vars.insert(name.to_string(), new);
    }

    /// Expand `$(VAR)`, `${VAR}` and `$X`; functions other than `$(shell cat FILE)` give
    /// [`UNKNOWN`].
    fn expand(&self, s: &str, depth: usize) -> String {
        if depth > 16 {
            return UNKNOWN.to_string();
        }
        let mut out = String::new();
        let mut chars = s.char_indices().peekable();
        while let Some((_, c)) = chars.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            let Some(&(i, open)) = chars.peek() else {
                break;
            };
            chars.next();
            let close = match open {
                '(' => ')',
                '{' => '}',
                '$' => {
                    out.push('$');
                    continue;
                }
                // `$@`, `$<`, `$^`, `$X`: automatic or one-letter variables.
                c => {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        out.push_str(&self.var(&c.to_string(), depth));
                    }
                    continue;
                }
            };
            // Find the matching close, counting nested opens.
            let body_start = i + 1;
            let mut level = 1;
            let mut end = s.len();
            for (j, ch) in chars.by_ref() {
                if ch == open {
                    level += 1;
                } else if ch == close {
                    level -= 1;
                    if level == 0 {
                        end = j;
                        break;
                    }
                }
            }
            let body = &s[body_start..end.min(s.len())];
            out.push_str(&self.reference(body, depth));
        }
        out
    }

    fn reference(&self, body: &str, depth: usize) -> String {
        let body_t = body.trim();
        if let Some(cmd) = body_t.strip_prefix("shell ") {
            return self.shell(&self.expand(cmd, depth + 1));
        }
        if body_t.contains(char::is_whitespace) || body_t.contains(':') {
            // `$(addprefix ...)`, `$(wildcard ...)`, substitution references...
            return UNKNOWN.to_string();
        }
        let name = self.expand(body_t, depth + 1);
        self.var(&name, depth)
    }

    fn var(&self, name: &str, depth: usize) -> String {
        // Unset is empty, as in make run without those variables in its environment.
        self.vars
            .get(name)
            .map_or_else(String::new, |v| self.expand(v, depth + 1))
    }

    /// `$(shell cat FILE)` is read here (newlines become spaces, as make does) and
    /// `$(shell uname)` answered from the host; nothing is ever run.
    fn shell(&self, cmd: &str) -> String {
        let words: Vec<&str> = cmd.split_whitespace().collect();
        if let ["cat", file] = words[..]
            && let Ok(p) = std::fs::canonicalize(self.root.join(file))
            && p.starts_with(self.root)
            && let Ok(text) = std::fs::read_to_string(&p)
        {
            return text.split_whitespace().collect::<Vec<_>>().join(" ");
        }
        // `ifeq ($(shell uname),Darwin)`: the host's name, as uname would print it.
        if let ["uname"] | ["uname", "-s"] = words[..] {
            return match std::env::consts::OS {
                "macos" => "Darwin".into(),
                "linux" => "Linux".into(),
                "freebsd" => "FreeBSD".into(),
                _ => UNKNOWN.to_string(),
            };
        }
        UNKNOWN.to_string()
    }
}

/// Lines with backslash-newline continuations joined.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    for line in text.lines() {
        if let Some(head) = line.strip_suffix('\\') {
            cur.push_str(head);
            cur.push(' ');
        } else {
            cur.push_str(line);
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn strip_comment(line: &str) -> &str {
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match c {
            '\\' => escaped = !escaped,
            '#' if !escaped => return &line[..i],
            _ => escaped = false,
        }
    }
    line
}

/// `[override|export] NAME op value` → (NAME, op, value).
fn assignment(line: &str) -> Option<(&str, &str, &str)> {
    let line = line
        .strip_prefix("override ")
        .or_else(|| line.strip_prefix("export "))
        .unwrap_or(line)
        .trim_start();
    let eq = line.find('=')?;
    let head = &line[..eq];
    let op_start = head
        .rfind(|c: char| !matches!(c, ':' | '+' | '?' | '!'))
        .map_or(0, |i| i + 1);
    let name = head[..op_start].trim();
    let op = &line[op_start..=eq];
    if name.is_empty()
        || name.contains([':', '#', '='])
        || name.contains(char::is_whitespace)
        || !matches!(op, "=" | ":=" | "::=" | ":::=" | "+=" | "?=" | "!=")
    {
        return None;
    }
    if op == "!=" {
        // `VAR != command` runs a shell; its value is unknown.
        return Some((name, "=", "\u{1}"));
    }
    Some((name, op, &line[eq + 1..]))
}

/// `(a,b)`, `"a" "b"` or `'a' 'b'`.
fn comparison(rest: &str) -> Option<(String, String)> {
    let rest = rest.trim();
    if let Some(inner) = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        // Split at the top-level comma.
        let mut level = 0;
        for (i, c) in inner.char_indices() {
            match c {
                '(' | '{' => level += 1,
                ')' | '}' => level -= 1,
                ',' if level == 0 => {
                    return Some((inner[..i].to_string(), inner[i + 1..].to_string()));
                }
                _ => {}
            }
        }
        return None;
    }
    let mut parts = vec![];
    let mut chars = rest.chars();
    while let Some(q) = chars.by_ref().find(|c| *c == '"' || *c == '\'') {
        parts.push(chars.by_ref().take_while(|c| *c != q).collect::<String>());
    }
    match &parts[..] {
        [a, b] => Some((a.clone(), b.clone())),
        _ => None,
    }
}

/// The preprocessor flags among shell words: `-D`/`-U` (unknown parts dropped), `-I` and
/// `-include` (made absolute; skipped when unknown).
pub fn compiler_flags(text: &str, root: &Path) -> Vec<String> {
    let abs = |p: &str| {
        let p = Path::new(p);
        if p.is_absolute() {
            p.display().to_string()
        } else {
            root.join(p).display().to_string()
        }
    };
    let mut out = vec![];
    let mut words = shell_words(text).into_iter();
    while let Some(w) = words.next() {
        if w == "-I" || w == "-include" || w == "-D" || w == "-U" {
            let Some(next) = words.next() else { break };
            if w == "-D" || w == "-U" {
                out.push(format!("{w}{}", next.replace(UNKNOWN, "")));
            } else if !next.contains(UNKNOWN) {
                out.push(if w == "-I" {
                    format!("-I{}", abs(&next))
                } else {
                    format!("-include{}", abs(&next))
                });
            }
        } else if let Some(p) = w.strip_prefix("-I") {
            if !p.contains(UNKNOWN) {
                out.push(format!("-I{}", abs(p)));
            }
        } else if let Some(d) = w.strip_prefix("-D").or_else(|| w.strip_prefix("-U")) {
            let d = d.replace(UNKNOWN, "");
            let name_ok = d.split('=').next().is_some_and(|n| {
                !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '_')
            });
            if name_ok {
                out.push(format!("{}{d}", &w[..2]));
            }
        }
    }
    out
}

/// Split like `sh` does: whitespace separates words, quotes group, backslash escapes.
fn shell_words(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\\' => {
                in_word = true;
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '\'' => {
                in_word = true;
                cur.extend(chars.by_ref().take_while(|&c| c != '\''));
            }
            '"' => {
                in_word = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some(n @ ('"' | '\\' | '$' | '`')) => cur.push(n),
                            Some(n) => {
                                cur.push('\\');
                                cur.push(n);
                            }
                            None => {}
                        },
                        c => cur.push(c),
                    }
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str, files: &[(&str, &str)]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("argo-make-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for (n, t) in files {
            std::fs::write(d.join(n), t).unwrap();
        }
        std::fs::canonicalize(d).unwrap()
    }

    #[test]
    fn later_recipe_lines_keep_a_rules_flags() {
        let d = dir("recipe", &[("cpu.c", ""), ("Makefile", "cpu64.o: cpu.c\n\t$(CC) -DMAX_XLEN=64 -c -o $@ $<\n\t@size $@\n")]);
        let m = read(&d);
        assert_eq!(m.per_file, [(d.join("cpu.c"), vec!["-DMAX_XLEN=64".to_string()])]);
    }

    #[test]
    fn tab_indented_lines_outside_a_rule_are_makefile_text() {
        let d = dir(
            "tabs",
            &[("Makefile", "ifeq (a,a)\n\tCFLAGS += -DLINUX\n\tifeq (a,b)\n\t\tCFLAGS += -DNO\n\tendif\nendif\nCFLAGS += -DAFTER\n")],
        );
        assert_eq!(read(&d).flags, ["-DLINUX".to_string(), "-DAFTER".into()]);
    }

    #[test]
    fn skips_make_syntax_it_cannot_evaluate() {
        let d = dir(
            "basic",
            &[(
                "Makefile",
                "CFLAGS=-O2 -Wall -D_GNU_SOURCE -DVERSION=\\\"$(shell git describe)\\\"\nCFLAGS+=-DCONFIG_X -I inc\nLDFLAGS=-DNOT_THIS\n",
            )],
        );
        assert_eq!(
            read(&d).flags,
            [
                "-D_GNU_SOURCE".to_string(),
                "-DVERSION=\"\"".into(),
                "-DCONFIG_X".into(),
                format!("-I{}", d.join("inc").display())
            ]
        );
    }

    /// TinyEMU's shape: feature switches, `ifdef`/`else` blocks adding defines, a version read
    /// with `$(shell cat VERSION)`, and one source compiled by several rules with its own `-D`.
    #[test]
    fn conditionals_shell_cat_and_per_file_rules() {
        let d = dir(
            "tinyemu",
            &[
                ("VERSION", "2019-12-21\n"),
                ("cpu.c", ""),
                (
                    "Makefile",
                    "# switches\nCONFIG_NET=y\n#CONFIG_WIN32=y\nCONFIG_WIDE=y\n\
                     ifdef CONFIG_WIN32\nCROSS=x-\nelse\nCROSS=\nendif\n\
                     CC=$(CROSS)gcc\nCFLAGS=-O2 -D_GNU_SOURCE -DCONFIG_VERSION=\\\"$(shell cat VERSION)\\\"\n\
                     ifdef CONFIG_NET\nCFLAGS+=-DCONFIG_NET\nendif\n\
                     ifdef CONFIG_WIN32\nCFLAGS+=-DCONFIG_WIN32\nendif\n\
                     ifdef CONFIG_WIDE\nCFLAGS+=-DMAX=128\nelse\nCFLAGS+=-DMAX=64\nendif\n\
                     ifeq ($(CROSS),)\nCFLAGS+=-DNATIVE\nendif\n\
                     OBJS=a.o \\\n  b.o\n\
                     cpu32.o: cpu.c\n\t$(CC) $(CFLAGS) -DXLEN=32 -c -o $@ $<\n\
                     cpu128.o: cpu.c\n\t$(CC) $(CFLAGS) -DXLEN=128 -c -o $@ $<\n\
                     %.o: %.c\n\t$(CC) $(CFLAGS) -DNOT_PER_FILE -c -o $@ $<\n",
                ),
            ],
        );
        let m = read(&d);
        assert_eq!(
            m.flags,
            [
                "-D_GNU_SOURCE",
                "-DCONFIG_VERSION=\"2019-12-21\"",
                "-DCONFIG_NET",
                "-DMAX=128",
                "-DNATIVE"
            ]
        );
        assert_eq!(m.optional, ["-DCONFIG_NET", "-DMAX=128", "-DNATIVE"]);
        assert_eq!(m.flags_for(&d.join("cpu.c")), ["-DXLEN=128"]);
    }

    #[test]
    fn shell_words_unquote() {
        assert_eq!(
            shell_words(r#"-DA=\"x\" '-DB=y z' "-DC=\"q\"" -DD"#),
            ["-DA=\"x\"", "-DB=y z", "-DC=\"q\"", "-DD"]
        );
    }
}
