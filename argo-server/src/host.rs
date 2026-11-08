//! The local SpacetimeDB host: a `spacetime start` child (which execs the standalone host in
//! place) on 127.0.0.1 with its own data directory.
//!
//! It runs [`isolate`]d, and `argo-server` stops it deliberately: SIGINT (the host's clean
//! shutdown), then SIGKILL if it lingers or a second stop signal arrives. A task owns the child
//! and publishes its exit, so every step can race against the host dying (`&self` methods).
//! `kill_on_drop` covers a panic.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::watch;

use crate::group::{isolate, signal};
use crate::interrupts::Interrupts;

const READY_WITHIN: Duration = Duration::from_secs(30);
const STOP_WITHIN: Duration = Duration::from_secs(10);

pub struct Host {
    pid: u32,
    port: u16,
    /// How the host exited, once it has.
    exit: watch::Receiver<Option<String>>,
    /// The host's own output, kept out of the server's.
    log: PathBuf,
}

impl Host {
    /// Spawn the host. The port is checked first, so readiness can't be mistaken for another
    /// process already answering there.
    pub fn spawn(port: u16, data_dir: &Path) -> Result<Host, String> {
        std::net::TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| format!("127.0.0.1:{port} is not free: {e}"))?;
        std::fs::create_dir_all(data_dir)
            .map_err(|e| format!("cannot create {}: {e}", data_dir.display()))?;
        let log = data_dir.join("host.log");
        let out = File::create(&log).map_err(|e| format!("{}: {e}", log.display()))?;
        let err = out.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new("spacetime");
        cmd.args(["start", "--non-interactive", "--listen-addr"])
            .arg(format!("127.0.0.1:{port}"))
            .arg("--data-dir")
            .arg(data_dir)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .kill_on_drop(true);
        let mut child = isolate(&mut cmd)
            .spawn()
            .map_err(|e| format!("cannot run spacetime start: {e}"))?;
        let pid = child.id().ok_or("spacetime start exited at once")?;
        let (tx, exit) = watch::channel(None);
        tokio::spawn(async move {
            let how = child.wait().await;
            let how = how.map_or_else(|e| e.to_string(), |s| s.to_string());
            let _ = tx.send(Some(how));
        });
        Ok(Host {
            pid,
            port,
            exit,
            log,
        })
    }

    /// Wait until the host answers `/v1/ping`.
    pub async fn ready(&self) -> Result<(), String> {
        let deadline = Instant::now() + READY_WITHIN;
        loop {
            if self.exit.borrow().is_some() {
                return Err(self.exited().await);
            }
            if ping(self.port).await {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(format!(
                    "host not ready after {READY_WITHIN:?}; see {}",
                    self.log.display()
                ));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Resolves only when the host has exited, describing how.
    pub async fn exited(&self) -> String {
        let mut exit = self.exit.clone();
        let how = match exit.wait_for(Option::is_some).await {
            Ok(how) => how.clone().unwrap_or_default(),
            Err(_) => "its watcher ended".into(),
        };
        format!("spacetime host exited ({how}); see {}", self.log.display())
    }

    /// Stop the host and wait for it. Another SIGINT/SIGTERM while waiting kills it at once.
    pub async fn stop(self, interrupts: &mut Interrupts) {
        if self.exit.borrow().is_some() {
            return;
        }
        signal(self.pid, libc::SIGINT);
        tokio::select! {
            _ = self.exited() => return,
            _ = tokio::time::sleep(STOP_WITHIN) => {
                eprintln!("argo-server: host ignored SIGINT for {STOP_WITHIN:?}; killing it");
            }
            why = interrupts.recv() => eprintln!("argo-server: {why} again; killing the host"),
        }
        // The host may have exited in the same instant; its reaped group id could be reused.
        if self.exit.borrow().is_none() {
            signal(self.pid, libc::SIGKILL);
        }
        self.exited().await;
    }
}

async fn ping(port: u16) -> bool {
    let probe = async {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await?;
        s.write_all(b"GET /v1/ping HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
            .await?;
        let mut head = [0; 12];
        s.read_exact(&mut head).await?;
        std::io::Result::Ok(head.ends_with(b" 200"))
    };
    matches!(
        tokio::time::timeout(Duration::from_secs(1), probe).await,
        Ok(Ok(true))
    )
}
