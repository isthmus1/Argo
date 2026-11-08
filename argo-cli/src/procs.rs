//! The part binaries and the child processes started from them.
//!
//! Each child gets its own process group, so the terminal's Ctrl-C reaches only `argo`, which
//! then stops every child once (SIGINT; `argo-server` stops its host on it) and, on a second
//! Ctrl-C or after a grace period, kills them.

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::signal::unix::{self as sig, Signal, SignalKind};
use tokio::sync::oneshot;

/// The workspace this binary was built from, where `cargo build` brings the parts up to date.
const WORKSPACE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
const PARTS: [&str; 3] = ["argo-server", "argo-worker", "argo-ui"];
const STOP_WITHIN: Duration = Duration::from_secs(15);

/// Where the part binaries are: beside this executable, else on `PATH`.
pub struct Bins {
    dir: Option<PathBuf>,
}

impl Bins {
    /// Find the parts, building them first when this executable lives in the workspace's
    /// `target/<profile>/` (so a `cargo run -p argo-cli` never starts stale parts).
    pub fn locate(no_build: bool) -> Result<Bins, String> {
        let dir = std::env::current_exe()
            .ok()
            .and_then(|e| std::fs::canonicalize(e).ok())
            .and_then(|e| e.parent().map(Path::to_path_buf));
        if let (false, Some(dir)) = (no_build, &dir) {
            build(dir)?;
        }
        Ok(Bins { dir })
    }

    /// A command for part `name`.
    pub fn cmd(&self, name: &str) -> Command {
        let local = self
            .dir
            .as_ref()
            .map(|d| d.join(name))
            .filter(|p| p.is_file());
        let mut cmd = Command::new(local.unwrap_or_else(|| name.into()));
        cmd.stdin(Stdio::null()).kill_on_drop(true).process_group(0);
        // Linux: a child outlives no `argo` killed outright (SIGTERM is its clean stop too).
        #[cfg(target_os = "linux")]
        // SAFETY: prctl is async-signal-safe and touches no memory of ours.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        cmd
    }
}

/// `cargo build` the parts (and `argo-export`) in the profile of `dir`, if it is this
/// workspace's target directory.
fn build(dir: &Path) -> Result<(), String> {
    let Ok(target) = std::fs::canonicalize(Path::new(WORKSPACE).join("target")) else {
        return Ok(());
    };
    if dir.parent() != Some(target.as_path()) {
        return Ok(());
    }
    let profile = match dir.file_name().and_then(|n| n.to_str()) {
        Some("debug") => "dev",
        Some(p) => p,
        None => return Ok(()),
    };
    eprintln!("argo: bringing the parts up to date ({profile})");
    let mut cmd = std::process::Command::new(std::env::var("CARGO").unwrap_or("cargo".into()));
    cmd.current_dir(WORKSPACE)
        .args(["build", "--quiet", "--profile", profile]);
    for part in PARTS {
        cmd.args(["-p", part]);
    }
    let status = cmd.status().map_err(|e| format!("cannot run cargo: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "building the parts failed; see above (or pass --no-build)".into())
}

/// How a child ending affects the rest.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Its ending, however it ends, stops everything (the server, the GUI).
    Essential,
    /// Only failing stops everything (a server that exits once a replay is played).
    Finishes,
    /// Its ending is reported and nothing else (workers).
    Free,
}

/// Why a group is stopping.
pub enum Stop {
    Interrupted,
    Exited(String, ExitStatus),
}

struct Proc {
    name: String,
    child: Child,
    role: Role,
    done: bool,
}

pub struct Group {
    procs: Vec<Proc>,
    signals: Signals,
}

impl Group {
    pub fn new() -> Group {
        Group {
            procs: Vec::new(),
            signals: Signals::new(),
        }
    }

    pub fn spawn(&mut self, name: &str, mut cmd: Command, role: Role) -> Result<(), String> {
        let child = cmd
            .spawn()
            .map_err(|e| format!("cannot start {name}: {e}"))?;
        self.procs.push(Proc {
            name: name.into(),
            child,
            role,
            done: false,
        });
        Ok(())
    }

    /// Spawn a child whose stderr is passed through; the receiver fires at the first line
    /// containing one of `markers`.
    pub fn spawn_watched(
        &mut self,
        name: &str,
        mut cmd: Command,
        role: Role,
        markers: &'static [&'static str],
    ) -> Result<oneshot::Receiver<()>, String> {
        cmd.stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("cannot start {name}: {e}"))?;
        let err = child.stderr.take().expect("piped");
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let mut tx = Some(tx);
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("{line}");
                if markers.iter().any(|m| line.contains(m)) {
                    tx.take().map(|tx| tx.send(()));
                }
            }
        });
        self.procs.push(Proc {
            name: name.into(),
            child,
            role,
            done: false,
        });
        Ok(rx)
    }

    /// Wait for `fut`, unless Ctrl-C or a child ending stops the group first.
    pub async fn until<T>(&mut self, fut: impl Future<Output = T>) -> Result<T, Stop> {
        tokio::pin!(fut);
        loop {
            tokio::select! {
                v = &mut fut => return Ok(v),
                _ = self.signals.recv() => return Err(Stop::Interrupted),
                _ = tokio::time::sleep(Duration::from_millis(200)) => {
                    if let Some(stop) = self.check() {
                        return Err(stop);
                    }
                }
            }
        }
    }

    /// Run until Ctrl-C or a child ending stops the group, then stop the rest.
    pub async fn supervise(mut self) -> Result<(), String> {
        let stop = match self.until(std::future::pending::<Infallible>()).await {
            Ok(never) => match never {},
            Err(stop) => stop,
        };
        self.shutdown(stop).await
    }

    /// The first child that ended in a way that stops the group; others are reported.
    fn check(&mut self) -> Option<Stop> {
        let mut stop = None;
        let mut last = None;
        for p in self.procs.iter_mut().filter(|p| !p.done) {
            let Ok(Some(status)) = p.child.try_wait() else {
                continue;
            };
            p.done = true;
            let stops = match p.role {
                Role::Essential => true,
                Role::Finishes => !status.success(),
                Role::Free => false,
            };
            if stops && stop.is_none() {
                stop = Some(Stop::Exited(p.name.clone(), status));
            } else {
                eprintln!("argo: {} ended ({status})", p.name);
            }
            last = Some((p.name.clone(), status));
        }
        // Nothing left running stops the group too.
        match (stop, last) {
            (Some(stop), _) => Some(stop),
            (None, Some((name, status))) if self.procs.iter().all(|p| p.done) => {
                Some(Stop::Exited(name, status))
            }
            _ => None,
        }
    }

    /// Stop every child that is still running, then report why the group stopped.
    pub async fn shutdown(mut self, stop: Stop) -> Result<(), String> {
        let outcome = match &stop {
            Stop::Interrupted => {
                eprintln!("argo: stopping");
                Ok(())
            }
            Stop::Exited(name, status) if status.success() => {
                eprintln!("argo: {name} ended; stopping the rest");
                Ok(())
            }
            Stop::Exited(name, status) => Err(format!("{name} failed ({status})")),
        };
        for p in self.procs.iter().filter(|p| !p.done) {
            signal(&p.child, libc::SIGINT);
        }
        let killed = {
            let signals = &mut self.signals;
            let procs = &mut self.procs;
            let all = async {
                for p in procs.iter_mut().filter(|p| !p.done) {
                    let _ = p.child.wait().await;
                }
            };
            tokio::select! {
                _ = all => false,
                _ = signals.recv() => true,
                _ = tokio::time::sleep(STOP_WITHIN) => true,
            }
        };
        if !killed {
            return outcome;
        }
        eprintln!("argo: still running; killing");
        for p in self.procs.iter_mut() {
            signal(&p.child, libc::SIGKILL);
            let _ = p.child.wait().await;
        }
        outcome
    }
}

/// Ctrl-C, or the terminal closing, or a SIGTERM: anything that would end `argo` before it
/// could stop its children. Listened to for the group's whole life, so none is missed.
struct Signals {
    int: Signal,
    term: Signal,
    hup: Signal,
}

impl Signals {
    fn new() -> Signals {
        let on = |k| sig::signal(k).expect("signal handlers install on Unix");
        Signals {
            int: on(SignalKind::interrupt()),
            term: on(SignalKind::terminate()),
            hup: on(SignalKind::hangup()),
        }
    }

    async fn recv(&mut self) {
        tokio::select! {
            _ = self.int.recv() => {}
            _ = self.term.recv() => {}
            _ = self.hup.recv() => {}
        }
    }
}

/// Signal a child's whole process group (its own compilers and the like too).
fn signal(child: &Child, sig: libc::c_int) {
    if let Some(pid) = child.id() {
        // SAFETY: kill(2) has no memory effects; a gone group is just an error.
        unsafe { libc::kill(-(pid as libc::pid_t), sig) };
    }
}
