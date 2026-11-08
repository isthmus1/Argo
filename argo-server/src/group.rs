//! Child processes in their own process group, so stopping one also stops everything it started
//! (the host behind `spacetime start`, cargo under `spacetime publish`, `cc`/`git` under the
//! repository loader), and a terminal Ctrl-C reaches only `argo-server`, which then decides.
//!
//! Process groups and signals are Unix; that is why `argo-server` is Unix-only (see `main.rs`).

use tokio::process::{Child, Command};

/// Give `cmd` its own process group. On Linux the kernel also SIGKILLs the child if
/// `argo-server`'s spawning thread dies first (see [`die_with_parent`]). Elsewhere a hard kill
/// of `argo-server` (SIGKILL, OOM) leaves its children running: [`crate::host::Host::spawn`]
/// then reports a host left on the port rather than starting a second one.
pub fn isolate(cmd: &mut Command) -> &mut Command {
    cmd.process_group(0);
    #[cfg(target_os = "linux")]
    die_with_parent(cmd);
    cmd
}

/// `PR_SET_PDEATHSIG`: covers the one exit no handler or destructor sees, `argo-server` itself
/// being SIGKILLed. Spawns happen on the main thread, which lives until exit. It reaches only
/// this direct child: a `git` or `cc` the loader was running (`cc` is spawned inside
/// `argo::project`, out of reach here) finishes on its own and exits. Linux-only; macOS and the
/// BSDs have no equivalent, so there it is simply not set.
#[cfg(target_os = "linux")]
fn die_with_parent(cmd: &mut Command) {
    // SAFETY: runs in the forked child before exec; `prctl` is async-signal-safe and touches no
    // memory of ours.
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        })
    };
}

/// Signal the process group led by `pid` (an [`isolate`]d child).
pub fn signal(pid: u32, sig: libc::c_int) {
    let group = -(pid as libc::pid_t);
    // SAFETY: `kill` takes plain integers and touches no memory; at worst the group is gone and
    // it returns ESRCH, which is fine here.
    unsafe { libc::kill(group, sig) };
}

/// SIGKILLs an [`isolate`]d child's whole group when dropped: a cancelled or failed step leaves
/// nothing running. [`Group::reaped`] disarms it once the child has been waited for, after which
/// its pid (and so the group id) may be reused.
pub struct Group(Option<u32>);

impl Group {
    pub fn of(child: &Child) -> Group {
        Group(child.id())
    }

    pub fn reaped(mut self) {
        self.0 = None;
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            signal(pid, libc::SIGKILL);
        }
    }
}
