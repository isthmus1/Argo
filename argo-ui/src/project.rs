//! An opened C project: loading it in the background (clone, preprocess, parse, export), the
//! file/function index the explorer shows, and the dialogs around it.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, channel};

use argo_c::project::{self as cproj, Opened, Options, Step, Unit, git::GitRef};
use argo_protocol::{NodeId, NodeKind, Snapshot};
use rune_ui::{TextEdit, Waker};

use crate::app::State;

/// Wakes the window when a background load reports progress.
pub static WAKER: OnceLock<Waker> = OnceLock::new();

pub struct Project {
    pub name: String,
    /// What was opened: a git reference or a local directory.
    pub source: String,
    /// Where the code was read: the local checkout, or (SpacetimeDB mode) the recorded commit.
    pub checkout: String,
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

/// Editable fields and choices behind the dialogs and the explorer, kept across reloads.
pub struct Forms {
    pub url: TextEdit,
    pub rev: TextEdit,
    pub flags: TextEdit,
    pub filter: TextEdit,
    pub error: Option<String>,
    pub scope: Scope,
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
            scope: Scope::Function,
            expanded: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modal {
    Open,
    Loading,
    Rewrite,
    NotImplemented,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Function,
    File,
    Project,
}

/// A load running on a background thread.
pub struct Loader {
    rx: Receiver<Msg>,
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

fn options(flags: &str) -> Options {
    Options {
        flags: flags.split_whitespace().map(str::to_string).collect(),
        ..Options::default()
    }
}

fn run(
    spec: &str,
    rev: &str,
    flags: &str,
    progress: &(dyn Fn(Step) + Sync),
) -> Result<Opened, String> {
    let (local, r) = target(spec, rev)?;
    let opts = options(flags);
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
        s.forms.error = Some(e);
        return;
    }
    let (tx, rx) = channel();
    let title = GitRef::parse(spec.trim()).name();
    let source = spec.trim().to_string();
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
        let result = run(&spec, &rev, &flags, &progress);
        let _ = tx.send(Msg::Done(Box::new(result), source));
        wake();
    });
    s.forms.error = None;
    s.loader = Some(Loader {
        rx,
        title: format!("Opening {title}"),
        step: "Starting".into(),
        progress: None,
    });
    s.modal = Some(Modal::Loading);
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
                s.forms.error = Some(e);
                s.modal = Some(Modal::Open);
            }
        }
    }
    changed
}

/// Open synchronously (command line and headless captures).
pub fn open_now(s: &mut State, spec: &str, rev: &str, flags: &str) -> Result<(), String> {
    let opened = run(spec, rev, flags, &|_| {})?;
    install(s, opened, spec.trim().to_string());
    Ok(())
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
    let first = entry(&modules);
    let forms = std::mem::take(&mut s.forms);
    let mut next = State::from_snapshot("project", snap, None, HashSet::new(), vec![]);
    next.forms = forms;
    next.project = Some(Project {
        name,
        source,
        checkout: opened.root.display().to_string(),
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

/// The function to show first: `main` in a .c file if there is one, else the first function of
/// the first .c file, else the first function.
pub(crate) fn entry(modules: &[Module]) -> Option<NodeId> {
    let c_files = || {
        modules
            .iter()
            .filter(|m| m.name.ends_with(".c"))
            .flat_map(|m| &m.functions)
    };
    c_files()
        .find(|(_, n)| n == "main")
        .or_else(|| c_files().next())
        .or_else(|| modules.iter().flat_map(|m| &m.functions).next())
        .map(|(id, _)| *id)
}

/// Modules (files) that define functions, with their functions, sorted by path.
pub(crate) fn index(snap: &Snapshot) -> Vec<Module> {
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
