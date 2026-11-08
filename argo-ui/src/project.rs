//! An opened C project: loading it in the background (clone, preprocess, parse, export), the
//! file/function index the explorer shows, and the dialogs around it.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, OnceLock};

use argo_c::project::git::{self, GitRef};
use argo_c::project::{self as cproj, Opened, Options, Step, Unit};
use argo_protocol::{NodeId, NodeKind, Snapshot};
use rune_ui::{TextEdit, Waker};

use crate::app::State;

/// Wakes the window when a background load reports progress.
pub static WAKER: OnceLock<Waker> = OnceLock::new();

pub struct Project {
    pub name: String,
    /// What was opened: a git reference or a local directory.
    pub source: String,
    pub root: PathBuf,
    pub units: Vec<Unit>,
    pub flags: Vec<String>,
    /// Modules (original files) that define functions, sorted by path.
    pub modules: Vec<Module>,
    /// File-level items that aren't functions (globals, types), counted from the graph.
    pub declarations: usize,
}

pub struct Module {
    pub id: NodeId,
    pub name: String,
    pub functions: Vec<(NodeId, String)>,
}

impl Project {
    pub fn function_count(&self) -> usize {
        self.modules.iter().map(|m| m.functions.len()).sum()
    }

    pub fn included(&self) -> usize {
        self.units.iter().filter(|u| u.skipped.is_none()).count()
    }

    pub fn module_of(&self, f: NodeId) -> Option<&Module> {
        self.modules
            .iter()
            .find(|m| m.functions.iter().any(|(id, _)| *id == f))
    }

    /// What the explorer says for a project with no function bodies: what it does hold.
    pub fn empty_note(&self) -> String {
        match self.declarations {
            0 => "Nothing declared".into(),
            1 => "1 declaration, no function bodies".into(),
            n => format!("{n} declarations, no function bodies"),
        }
    }
}

/// Editable fields behind the dialogs and the explorer, kept across reloads.
pub struct Forms {
    pub url: TextEdit,
    pub rev: TextEdit,
    pub flags: TextEdit,
    pub filter: TextEdit,
    pub error: Option<String>,
    /// The open dialog lists every line of `error`, not just the first few.
    pub error_open: bool,
    /// The module expanded in the explorer.
    pub expanded: Option<NodeId>,
}

impl Default for Forms {
    fn default() -> Self {
        Forms {
            url: TextEdit::new(""),
            rev: TextEdit::new(""),
            flags: TextEdit::new(""),
            filter: TextEdit::new(""),
            error: None,
            error_open: false,
            expanded: None,
        }
    }
}

impl Forms {
    pub(crate) fn set_error(&mut self, e: Option<String>) {
        self.error = e;
        self.error_open = false;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modal {
    Open,
    Loading,
}

/// What the open dialog shows after the user cancels a load.
pub const CANCELLED: &str = "Opening was cancelled.";

/// A load running on a background thread.
pub struct Loader {
    rx: Receiver<Msg>,
    /// Set to stop the thread: a running git command is killed, unread files are skipped.
    cancel: Arc<AtomicBool>,
    pub title: String,
    pub step: String,
    pub progress: Option<(usize, usize)>,
}

enum Msg {
    Step(Step),
    Done(Box<Result<Opened, String>>, String),
}

/// What a spec opens.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    Local(PathBuf),
    Git(GitRef),
}

/// `spec` with a leading `~` or `~/` replaced by `$HOME`.
fn expand_home(spec: &str) -> PathBuf {
    let rest = if spec == "~" { Some("") } else { spec.strip_prefix("~/") };
    match (rest, std::env::var("HOME")) {
        (Some(rest), Ok(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(spec),
    }
}

/// Whether `spec` names a local folder rather than a git remote: it starts like a path (`/`,
/// `~`, `./`, `../`), or it is no URL (`://`, `git@`) and something by that name exists.
pub fn path_shaped(spec: &str) -> bool {
    let spec = spec.trim();
    if ["/", "~", "./", "../"].iter().any(|p| spec.starts_with(p)) {
        return true;
    }
    !spec.is_empty()
        && !spec.contains("://")
        && !spec.contains("git@")
        && std::fs::symlink_metadata(expand_home(spec)).is_ok()
}

/// What to open: a local folder, or a git reference (with the revision field overriding one
/// written into the URL). A path-shaped spec that names no readable folder fails here, before
/// any git work, saying why; one with a revision after it (`~/src/repo#v1`) is a local
/// repository at that revision.
fn target(spec: &str, rev: &str) -> Result<Target, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("Enter a git URL or a local directory.".into());
    }
    let mut r = GitRef::parse(spec);
    if !rev.trim().is_empty() {
        r.rev = Some(rev.trim().to_string());
    }
    if !path_shaped(spec) {
        return Ok(Target::Git(r));
    }
    let path = expand_home(spec);
    match std::fs::metadata(&path) {
        Ok(m) if m.is_dir() => match std::fs::read_dir(&path) {
            Ok(_) => Ok(Target::Local(path)),
            Err(e) => Err(format!("Can't read {spec}: {e}")),
        },
        Ok(_) => Err(format!("Not a folder: {spec}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if r.url != spec && expand_home(&r.url).is_dir() {
                Ok(Target::Git(GitRef { url: expand_home(&r.url).display().to_string(), ..r }))
            } else {
                Err(format!("No such folder: {spec}"))
            }
        }
        Err(e) => Err(format!("Can't read {spec}: {e}")),
    }
}

fn options(flags: &str, cancel: Arc<AtomicBool>) -> Options {
    Options {
        flags: flags.split_whitespace().map(str::to_string).collect(),
        cancel,
        ..Options::default()
    }
}

fn run(
    spec: &str,
    rev: &str,
    flags: &str,
    cancel: Arc<AtomicBool>,
    progress: &(dyn Fn(Step) + Sync),
) -> Result<Opened, String> {
    let opts = options(flags, cancel);
    match target(spec, rev)? {
        Target::Local(dir) => {
            let name = std::fs::canonicalize(&dir)
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "project".into());
            cproj::open(&dir, &name, &opts, progress)
        }
        Target::Git(r) => cproj::open_git(&r, &opts, progress),
    }
}

/// Open the project named by the dialog's fields on a background thread.
pub fn start(s: &mut State) {
    let (spec, rev, flags) = (
        s.forms.url.text.clone(),
        s.forms.rev.text.clone(),
        s.forms.flags.text.clone(),
    );
    if let Err(e) = target(&spec, &rev) {
        s.forms.set_error(Some(e));
        return;
    }
    let (tx, rx) = channel();
    let title = GitRef::parse(spec.trim()).name();
    let source = spec.trim().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    std::thread::spawn(move || {
        let wake = || {
            if let Some(w) = WAKER.get() {
                w();
            }
        };
        let progress = |step: Step| {
            let _ = tx.send(Msg::Step(step));
            wake();
        };
        let result = run(&spec, &rev, &flags, stop, &progress);
        log_outcome(&source, result.as_ref().map_err(String::as_str));
        let _ = tx.send(Msg::Done(Box::new(result), source));
        wake();
    });
    s.forms.set_error(None);
    s.loader = Some(Loader {
        rx,
        cancel,
        title: format!("Opening {title}"),
        step: "Starting".into(),
        progress: None,
    });
    s.modal = Some(Modal::Loading);
}

/// The progress dialog at `step` with no count yet and no thread behind it, for the
/// `project-loading` review scene.
pub fn show_loading(s: &mut State, step: &str) {
    let (_, rx) = channel();
    s.loader = Some(Loader {
        rx,
        cancel: Arc::default(),
        title: "Opening TinyEMU".into(),
        step: step.into(),
        progress: None,
    });
    s.modal = Some(Modal::Loading);
}

/// Stop the running load and go back to the open dialog, its fields as they were, so it can be
/// retried. The thread stops on its own shortly after (killing a running git command); what it
/// reports is dropped.
pub fn cancel(s: &mut State) {
    let Some(loader) = s.loader.take() else {
        return;
    };
    loader.cancel.store(true, Ordering::Relaxed);
    s.forms.set_error(Some(CANCELLED.into()));
    s.modal = Some(Modal::Open);
}

/// Apply background progress; true when something changed.
pub fn poll(s: &mut State) -> bool {
    let Some(loader) = &mut s.loader else {
        return false;
    };
    let mut changed = false;
    let mut done = None;
    while let Ok(msg) = loader.rx.try_recv() {
        changed = true;
        match msg {
            Msg::Step(step) => {
                loader.progress = None;
                loader.step = match step {
                    Step::Fetching(m) => m,
                    Step::Scanning => "Finding .c files".into(),
                    Step::Checking { done, total } => {
                        loader.progress = Some((done, total));
                        format!("Preprocessing and parsing each file · {done} / {total}")
                    }
                    Step::Combining { units } => {
                        format!("Combining {units} files into one translation unit")
                    }
                    Step::Parsing => "Parsing the combined source".into(),
                    Step::Building => "Building the graph".into(),
                };
            }
            Msg::Done(result, source) => done = Some((*result, source)),
        }
    }
    if let Some((result, source)) = done {
        s.loader = None;
        match result {
            Ok(opened) => install(s, opened, source),
            Err(e) => {
                let e = if e == git::CANCELLED { CANCELLED.into() } else { e };
                s.forms.set_error(Some(e));
                s.modal = Some(Modal::Open);
            }
        }
    }
    changed
}

/// Open synchronously (command line and headless captures).
pub fn open_now(s: &mut State, spec: &str, rev: &str, flags: &str) -> Result<(), String> {
    let opened = run(spec, rev, flags, Arc::default(), &|_| {})?;
    // An error goes back to the caller, which prints it.
    log_outcome(spec.trim(), Ok(&opened));
    install(s, opened, spec.trim().to_string());
    Ok(())
}

/// Print what happened to stderr: the error in full (every file and its reason when nothing
/// could be read), or the files left out of a successful load and the ones read with flags of
/// their own.
fn log_outcome(source: &str, result: Result<&Opened, &str>) {
    eprint!("{}", outcome_text(source, result));
}

fn outcome_text(source: &str, result: Result<&Opened, &str>) -> String {
    let opened = match result {
        Err(e) => return format!("argo-ui: could not open {source}\n{}\n", indent(e)),
        Ok(o) => o,
    };
    let skipped: Vec<&Unit> = opened
        .units
        .iter()
        .filter(|u| u.skipped.is_some())
        .collect();
    let mut out = format!(
        "argo-ui: opened {source}: {} of {} .c files\n",
        opened.units.len() - skipped.len(),
        opened.units.len()
    );
    if !skipped.is_empty() {
        out.push_str(&format!("  left out ({}):\n", skipped.len()));
        for u in skipped {
            let why = u.skipped.as_deref().unwrap_or_default();
            out.push_str(&format!("    {}: {why}\n", u.path));
        }
    }
    for u in opened
        .units
        .iter()
        .filter(|u| u.skipped.is_none() && !u.flags.is_empty())
    {
        out.push_str(&format!("  read with {}: {}\n", u.flags.join(" "), u.path));
    }
    out
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the opener reports when nothing can be read (its real output on a nine-file fixture), for
/// the `project-open-error` review scene.
pub const SAMPLE_ERROR: &str = "None of the 9 .c files could be preprocessed and parsed.
fs_wget.c: openssl/argo_absent_aes.h: No such file or directory (fs_wget.c:1)
jsemu.c: emscripten.h: No such file or directory (jsemu.c:1)
machine.c: expected a name, found `^` (machine.c:1)
riscv_cpu.c: #error MAX_XLEN must be defined (riscv_cpu.c:2)
sdl.c: SDL/SDL.h: No such file or directory (sdl.c:1)
slirp/slirp.c: argo_absent/slirp_config.h: No such file or directory (slirp/slirp.c:1)
temu.c: expected `;`, found `CONFIG_VERSION` (temu.c:1)
virtio.c: expected `)`, found `{` (virtio.c:1)
x86_cpu.c: #error not a native build (x86_cpu.c:1)";

/// A load error split for the dialog: its headline, at most `cap` of the lines after it (one per
/// file when nothing could be read), and how many more there are.
pub fn error_parts(error: &str, cap: usize) -> (&str, Vec<&str>, usize) {
    let mut lines = error.lines().filter(|l| !l.trim().is_empty());
    let head = lines.next().unwrap_or("");
    let rest: Vec<&str> = lines.collect();
    let more = rest.len().saturating_sub(cap);
    (head, rest.into_iter().take(cap).collect(), more)
}

/// One error line as the dialog shows it: where (`path:line`, else the file) and why. The opener
/// writes `file.c: reason (file.c:line)`; the location moves to the front so it survives a cut,
/// and isn't repeated. A line with no `file: ` prefix is all reason.
pub fn error_row(line: &str) -> (&str, &str) {
    let Some((file, rest)) = line.split_once(": ") else {
        return ("", line);
    };
    if file.is_empty() || file.contains(char::is_whitespace) {
        return ("", line);
    }
    if let Some(open) = rest.rfind(" (")
        && let Some(at) = rest[open + 2..].strip_suffix(')')
        && at
            .strip_prefix(file)
            .and_then(|n| n.strip_prefix(':'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    {
        return (at, &rest[..open]);
    }
    (file, rest)
}

/// What a project with no function definitions holds, for the `project-empty` review scene.
const SAMPLE_EMPTY: &str = "int verbose = 0;\nstruct options { int level; const char *name; };\nextern int limit;\n";

/// Open a real project whose one file declares but defines no functions (written to a temporary
/// folder), for the `project-empty` review scene.
pub fn open_sample_empty(s: &mut State) -> Result<(), String> {
    let dir = std::env::temp_dir()
        .join(format!("argo-ui-empty-{}", std::process::id()))
        .join("settings");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(dir.join("config.c"), SAMPLE_EMPTY).map_err(|e| format!("{}: {e}", dir.display()))?;
    open_now(s, &dir.display().to_string(), "", "")
}

/// Whether most of an error's file lines are a missing header or an undefined required macro:
/// the kind of failure an `-I` or `-D` in the preprocessor flags fixes.
pub fn needs_flags(error: &str) -> bool {
    let (_, files, _) = error_parts(error, usize::MAX);
    let fixable = files
        .iter()
        .filter(|l| l.contains("No such file") || (l.contains("#error") && l.contains("must be defined")))
        .count();
    fixable >= 2 && 2 * fixable > files.len()
}

/// Show an opened project: index its files and functions and open its entry point.
fn install(s: &mut State, opened: Opened, source: String) {
    let snap = opened.snapshot;
    let modules = index(&snap);
    let declarations = declarations(&snap);
    // With nothing to open, the canvas shows what there is: the one file's declarations, or
    // each file as a group of them.
    let level = file_level(&snap);
    let name = snap
        .graph
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Program)
        .map_or_else(|| "project".into(), |n| n.name.clone());
    // Start at `main` in a .c file if there is one, else the first function of the first .c file.
    let first = modules
        .iter()
        .filter(|m| m.name.ends_with(".c"))
        .flat_map(|m| &m.functions)
        .find(|(_, n)| n == "main")
        .or_else(|| {
            modules
                .iter()
                .filter(|m| m.name.ends_with(".c"))
                .flat_map(|m| &m.functions)
                .next()
        })
        .or_else(|| modules.iter().flat_map(|m| &m.functions).next())
        .map(|(id, _)| *id);
    let forms = std::mem::take(&mut s.forms);
    let mut next = State::from_snapshot("project", snap, None, HashSet::new(), vec![]);
    next.forms = forms;
    next.project = Some(Project {
        name,
        source,
        root: opened.root,
        units: opened.units,
        flags: opened.flags,
        modules,
        declarations,
    });
    next.modal = None;
    *s = next;
    if let Some(f) = first {
        open_function(s, f);
    } else if let Some(c) = level {
        show_level(s, c);
    }
}

/// File-level items that aren't functions: every non-function child of a module.
fn declarations(snap: &Snapshot) -> usize {
    let nodes = &snap.graph.nodes;
    let modules: HashSet<NodeId> = nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .map(|n| n.id)
        .collect();
    nodes
        .iter()
        .filter(|n| n.kind != NodeKind::Function && n.parent.is_some_and(|p| modules.contains(&p)))
        .count()
}

/// The container whose level lists a project's files' contents: its one module, else the
/// program (each module a group), else none.
fn file_level(snap: &Snapshot) -> Option<NodeId> {
    let nodes = &snap.graph.nodes;
    let mut modules = nodes.iter().filter(|n| n.kind == NodeKind::Module);
    match (modules.next(), modules.next()) {
        (Some(m), None) => Some(m.id),
        _ => nodes.iter().find(|n| n.kind == NodeKind::Program).map(|n| n.id),
    }
}

/// Show container `c`'s level with nothing inside it open.
fn show_level(s: &mut State, c: NodeId) {
    let Some(live) = &mut s.live else { return };
    live.container = Some(c);
    live.open.clear();
    s.selected = None;
    s.hovered = None;
    s.reproject();
    s.relayout();
    s.fit();
}

fn index(snap: &Snapshot) -> Vec<Module> {
    let nodes = &snap.graph.nodes;
    let mut modules: Vec<Module> = nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .map(|m| Module {
            id: m.id,
            name: m.name.clone(),
            functions: vec![],
        })
        .collect();
    for n in nodes.iter().filter(|n| n.kind == NodeKind::Function) {
        if let Some(m) = modules.iter_mut().find(|m| Some(m.id) == n.parent) {
            m.functions.push((n.id, n.name.clone()));
        }
    }
    modules.retain(|m| !m.functions.is_empty());
    // By path, so a file's header sits beside it and directories group together.
    modules.sort_by(|a, b| a.name.cmp(&b.name));
    modules
}

/// Show one function: its pins on the canvas, its body block open.
pub fn open_function(s: &mut State, f: NodeId) {
    let Some(live) = &mut s.live else { return };
    let body: HashSet<NodeId> = live
        .snap
        .graph
        .nodes
        .iter()
        .filter(|n| n.parent == Some(f) && n.kind == NodeKind::Scope)
        .map(|n| n.id)
        .collect();
    live.container = Some(f);
    live.open = body;
    if let Some(p) = &s.project {
        s.forms.expanded = p.module_of(f).map(|m| m.id);
    }
    s.selected = None;
    s.hovered = None;
    s.reproject();
    s.relayout();
    s.fit();
}

/// The function on the canvas, if a project is open.
pub fn current(s: &State) -> Option<(NodeId, String)> {
    let live = s.live.as_ref()?;
    let id = live.container?;
    let n = live.snap.graph.nodes.iter().find(|n| n.id == id)?;
    Some((id, n.name.clone()))
}

/// The open dialog's fields, prefilled from a command line `spec`.
pub fn prefill(forms: &mut Forms, spec: &str, flags: &[String]) {
    let r = GitRef::parse(spec);
    let local = path_shaped(spec) && expand_home(spec.trim()).is_dir();
    forms.url = TextEdit::new(if local {
        spec.to_string()
    } else {
        r.url.clone()
    });
    forms.rev = TextEdit::new(if local {
        String::new()
    } else {
        r.rev.unwrap_or_default()
    });
    forms.flags = TextEdit::new(flags.join(" "));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(s: &mut State) {
        let t = std::time::Instant::now();
        while s.loader.is_some() && t.elapsed().as_secs() < 60 {
            poll(s);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(s.loader.is_none(), "load finished");
    }

    #[test]
    fn a_background_load_installs_the_project_and_opens_a_function() {
        if !std::process::Command::new("cc")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!("argo-ui-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.c"), "int helper(int x) { return x + 1; }\nint main(void) { if (helper(1)) return 0; return 1; }\n").unwrap();
        std::fs::write(dir.join("b.c"), "#include <argo_missing.h>\n").unwrap();

        let mut s = State::load("fixture");
        s.forms.url = TextEdit::new(dir.display().to_string());
        start(&mut s);
        assert_eq!(s.modal, Some(Modal::Loading));
        wait(&mut s);

        assert_eq!(s.modal, None);
        let p = s.project.as_ref().expect("project installed");
        assert_eq!((p.included(), p.units.len(), p.function_count()), (1, 2, 2));
        assert_eq!(
            current(&s).map(|(_, n)| n).as_deref(),
            Some("main"),
            "opens main first"
        );
        assert_eq!(s.view.crumbs.last().map(String::as_str), Some("main"));
        assert!(
            s.view.nodes.iter().any(|n| n.title.starts_with("if (")),
            "the body is drawn"
        );

        let helper = p.modules[0]
            .functions
            .iter()
            .find(|(_, n)| n == "helper")
            .unwrap()
            .0;
        open_function(&mut s, helper);
        assert_eq!(current(&s).map(|(_, n)| n).as_deref(), Some("helper"));
        assert_eq!(
            s.forms.url.text,
            dir.display().to_string(),
            "the dialog keeps what was typed"
        );
    }

    #[test]
    fn a_cancelled_load_returns_to_the_dialog_ready_to_retry() {
        let has_cc = std::process::Command::new("cc")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
        let dir = std::env::temp_dir().join(format!("argo-ui-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.c"), "int main(void) { return 0; }\n").unwrap();

        let mut s = State::load("fixture");
        s.forms.url = TextEdit::new(dir.display().to_string());
        s.forms.flags = TextEdit::new("-DX=1");
        start(&mut s);
        assert_eq!(s.modal, Some(Modal::Loading));
        let flag = s.loader.as_ref().unwrap().cancel.clone();
        cancel(&mut s);

        assert!(flag.load(Ordering::Relaxed), "the thread is told to stop");
        assert!(s.loader.is_none());
        assert_eq!(s.modal, Some(Modal::Open));
        assert_eq!(s.forms.error.as_deref(), Some(CANCELLED));
        assert_eq!(s.forms.url.text, dir.display().to_string(), "the form is intact");
        assert_eq!(s.forms.flags.text, "-DX=1");
        assert!(s.project.is_none());
        // What the stopped thread sends later is dropped with its loader.
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!poll(&mut s));
        assert_eq!(s.modal, Some(Modal::Open));

        // Open again from the same fields.
        if has_cc {
            start(&mut s);
            assert_eq!(s.forms.error, None);
            wait(&mut s);
            assert_eq!(s.modal, None);
            assert!(s.project.is_some());
        }
    }

    #[test]
    fn a_cancel_reported_by_the_thread_reads_as_cancelled() {
        let (tx, rx) = channel();
        let mut s = State::load("fixture");
        s.loader = Some(Loader {
            rx,
            cancel: Arc::default(),
            title: String::new(),
            step: String::new(),
            progress: None,
        });
        s.modal = Some(Modal::Loading);
        tx.send(Msg::Done(Box::new(Err(git::CANCELLED.into())), "x".into()))
            .unwrap();
        poll(&mut s);
        assert_eq!(s.modal, Some(Modal::Open));
        assert_eq!(s.forms.error.as_deref(), Some(CANCELLED));
    }

    #[test]
    fn missing_headers_and_defines_point_at_the_flags() {
        assert!(needs_flags(SAMPLE_ERROR));
        assert!(!needs_flags(
            "None of the 3 .c files could be preprocessed and parsed.\na.c: No such file\nb.c: expected `;`\nc.c: expected `)`"
        ));
        assert!(!needs_flags("git fetch: timed out after 120 s"));
    }

    #[test]
    fn errors_split_into_a_headline_and_a_capped_file_list() {
        let e = "None of the 4 .c files could be preprocessed and parsed.\na.c: x (a.c:1)\nb.c: y (b.c:2)\nc.c: z\nd.c: w";
        assert_eq!(
            error_parts(e, 2),
            (
                "None of the 4 .c files could be preprocessed and parsed.",
                vec!["a.c: x (a.c:1)", "b.c: y (b.c:2)"],
                2
            )
        );
        assert_eq!(
            error_parts("fetch: denied", 6),
            ("fetch: denied", vec![], 0)
        );
    }

    #[test]
    fn an_error_row_leads_with_its_location() {
        assert_eq!(
            error_row("slirp/slirp.c: argo_absent/slirp_config.h: No such file or directory (slirp/slirp.c:1)"),
            ("slirp/slirp.c:1", "argo_absent/slirp_config.h: No such file or directory")
        );
        assert_eq!(
            error_row("riscv_cpu.c: #error MAX_XLEN must be defined (riscv_cpu.c:2)"),
            ("riscv_cpu.c:2", "#error MAX_XLEN must be defined")
        );
        // A location in another file, or none, stays in the reason under the file's name.
        assert_eq!(
            error_row("a.c: x (b.h:3)"),
            ("a.c", "x (b.h:3)")
        );
        assert_eq!(error_row("c.c: z"), ("c.c", "z"));
        assert_eq!(error_row("git fetch: timed out"), ("", "git fetch: timed out"));
        assert_eq!(error_row("plain words"), ("", "plain words"));
        // Every line of the sample keeps its file.
        let (_, files, _) = error_parts(SAMPLE_ERROR, usize::MAX);
        for l in files {
            let (at, why) = error_row(l);
            assert!(l.starts_with(at.split(':').next().unwrap()) && !at.is_empty(), "{l}");
            assert!(!why.ends_with(')'), "{l}");
        }
    }

    #[test]
    fn path_shaped_specs_fail_on_the_folder_before_any_git_work() {
        let base = std::env::temp_dir().join(format!("argo-ui-target-{}", std::process::id()));
        let dir = base.join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        let file = base.join("notes.txt");
        std::fs::write(&file, "x").unwrap();
        let d = dir.display().to_string();

        assert_eq!(target(&d, ""), Ok(Target::Local(dir.clone())));
        // A folder ignores the revision field.
        assert_eq!(target(&format!("  {d}  "), "v1"), Ok(Target::Local(dir.clone())));
        assert_eq!(
            target("/argo/nonexistent/dir", ""),
            Err("No such folder: /argo/nonexistent/dir".into())
        );
        assert_eq!(
            target("./argo-definitely-missing", "main"),
            Err("No such folder: ./argo-definitely-missing".into())
        );
        assert_eq!(
            target("~/argo-definitely-missing", ""),
            Err("No such folder: ~/argo-definitely-missing".into())
        );
        assert_eq!(
            target(&file.display().to_string(), ""),
            Err(format!("Not a folder: {}", file.display()))
        );
        // Below a file: not a missing folder, an error reading the path.
        let under = format!("{}/x", file.display());
        assert!(
            target(&under, "").is_err_and(|e| e.starts_with(&format!("Can't read {under}: "))),
            "{:?}",
            target(&under, "")
        );
        // A local repository with a revision after it is fetched at that revision.
        assert_eq!(
            target(&format!("{d}#v1"), ""),
            Ok(Target::Git(GitRef::new(&d, Some("v1"))))
        );

        // Remotes stay git references, the revision field overriding one in the URL.
        assert_eq!(
            target("https://github.com/a/b.git#v1", "main"),
            Ok(Target::Git(GitRef::new("https://github.com/a/b.git", Some("main"))))
        );
        assert_eq!(
            target("git@github.com:a/b.git", ""),
            Ok(Target::Git(GitRef::new("git@github.com:a/b.git", None)))
        );
        assert_eq!(
            target("file:///argo/x", ""),
            Ok(Target::Git(GitRef::new("file:///argo/x", None)))
        );
        assert!(target("   ", "").is_err());

        // What the dialog asks to style the revision field.
        assert!(path_shaped("/x") && path_shaped("~") && path_shaped("~/x") && path_shaped("../x"));
        assert!(!path_shaped("https://github.com/a/b") && !path_shaped("git@host:a/b"));
        assert!(!path_shaped("github.com/a/b") && !path_shaped(""));
        // A bare relative name is a folder when it exists (tests run in the crate's folder).
        assert!(path_shaped("src") && target("src", "").is_ok_and(|t| t == Target::Local("src".into())));
    }

    #[test]
    fn a_project_without_function_definitions_opens_empty() {
        if !std::process::Command::new("cc")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return;
        }
        let mut s = State::load("fixture");
        open_sample_empty(&mut s).expect("opens");
        let p = s.project.as_ref().expect("project installed");
        assert_eq!((p.included(), p.units.len(), p.function_count()), (1, 1, 0));
        assert!(p.modules.is_empty());
        assert_eq!(s.modal, None);
        // The explorer's count is what the canvas shows: the file's level, one node for each
        // declaration (`extern int limit;` defines nothing, so the graph has no node for it).
        assert_eq!(p.declarations, 2);
        assert_eq!(p.empty_note(), "2 declarations, no function bodies");
        assert_eq!(current(&s).map(|(_, n)| n).as_deref(), Some("config.c"));
        let mut titles: Vec<&str> = s.view.nodes.iter().map(|n| n.title.as_str()).collect();
        titles.sort();
        assert_eq!(titles, ["options", "verbose"]);
    }

    #[test]
    fn the_terminal_gets_every_left_out_file() {
        let err = "None of the 2 .c files could be preprocessed and parsed.\na.c: x\nb.c: y";
        assert_eq!(
            outcome_text("~/t", Err(err)),
            "argo-ui: could not open ~/t\n  None of the 2 .c files could be preprocessed and parsed.\n  a.c: x\n  b.c: y\n"
        );
    }

    #[test]
    fn a_failed_load_returns_to_the_dialog_with_the_error() {
        let mut s = State::load("fixture");
        start(&mut s);
        assert_eq!(s.modal, None, "an empty field never starts");
        assert!(s.forms.error.is_some());

        s.forms.url = TextEdit::new("file:///argo/definitely/not/a/repo");
        start(&mut s);
        wait(&mut s);
        assert_eq!(s.modal, Some(Modal::Open));
        assert!(
            s.forms.error.as_deref().is_some_and(|e| e.contains("git")),
            "{:?}",
            s.forms.error
        );
        assert!(s.project.is_none());
    }
}
