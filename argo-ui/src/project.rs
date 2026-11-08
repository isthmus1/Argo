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
    fn set_error(&mut self, e: Option<String>) {
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

/// What to open: a local directory, or a git reference (with the revision field overriding one
/// written into the URL).
fn target(spec: &str, rev: &str) -> Result<(Option<PathBuf>, GitRef), String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("Enter a git URL or a local directory.".into());
    }
    let path = PathBuf::from(spec.strip_prefix("~/").map_or(spec.to_string(), |rest| {
        std::env::var("HOME").map_or(spec.to_string(), |h| format!("{h}/{rest}"))
    }));
    let mut r = GitRef::parse(spec);
    if !rev.trim().is_empty() {
        r.rev = Some(rev.trim().to_string());
    }
    Ok((path.is_dir().then_some(path), r))
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
    let (local, r) = target(spec, rev)?;
    let opts = options(flags, cancel);
    match local {
        Some(dir) => {
            let name = std::fs::canonicalize(&dir)
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "project".into());
            cproj::open(&dir, &name, &opts, progress)
        }
        None => cproj::open_git(&r, &opts, progress),
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
    });
    next.modal = None;
    *s = next;
    if let Some(f) = first {
        open_function(s, f);
    }
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
    let local = PathBuf::from(spec).is_dir();
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
