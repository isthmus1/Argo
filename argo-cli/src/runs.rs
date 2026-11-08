//! The recorded runs on the local host: every database there with an ingested program.
//!
//! The host must be up to be asked, so when none answers on `--port` a temporary one is started
//! on the same data directory and stopped again before the replay starts its own.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};

use crate::args::Local;

/// One recorded run.
pub struct Run {
    pub name: String,
    pub locator: String,
    pub commit: String,
    pub rust: usize,
    pub exchanges: usize,
}

impl Run {
    /// One line of the picker.
    pub fn line(&self, width: usize) -> String {
        let repo = self.locator.rsplit('/').next().unwrap_or(&self.locator);
        let commit = self.commit.get(..7).unwrap_or(&self.commit);
        format!(
            "{:width$}  {repo} @{commit}  {} Rust, {} model exchanges",
            self.name, self.rust, self.exchanges
        )
    }
}

/// The host the runs were read from: one already running stays up; ours is stopped.
pub struct Host {
    pub url: String,
    temp: Option<Child>,
}

impl Host {
    pub fn ours(&self) -> bool {
        self.temp.is_some()
    }

    /// Stop the temporary host, if this started one, and wait for it.
    pub async fn release(mut self) {
        if let Some(child) = &mut self.temp {
            if let Some(pid) = child.id() {
                // SAFETY: kill(2) has no memory effects.
                unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGINT) };
            }
            let _ = tokio::time::timeout(Duration::from_secs(10), child.wait()).await;
        }
    }
}

/// The host on `local.port`, started from its data directory if nothing answers there.
pub async fn host(local: &Local) -> Result<Host, String> {
    let url = format!("http://127.0.0.1:{}", local.port);
    if ping(local.port) {
        return Ok(Host { url, temp: None });
    }
    let dir = data_dir(local);
    if !dir.is_dir() {
        return Err(format!(
            "no recordings yet: {} doesn't exist",
            dir.display()
        ));
    }
    let mut cmd = Command::new("spacetime");
    cmd.args(["start", "--non-interactive", "--listen-addr"])
        .arg(format!("127.0.0.1:{}", local.port))
        .arg("--data-dir")
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    let child = cmd
        .spawn()
        .map_err(|e| format!("cannot run spacetime start: {e}"))?;
    let host = Host {
        url,
        temp: Some(child),
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ping(local.port) {
        if Instant::now() > deadline {
            host.release().await;
            return Err("the SpacetimeDB host didn't come up within 30s".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Ok(host)
}

/// Every database on `host` with an ingested program, except `skip` (the replay's target).
pub async fn list(host: &Host, skip: &str) -> Result<Vec<Run>, String> {
    let out = spacetime(&["list", "-y", "--server", &host.url]).await?;
    let ids: Vec<String> = out
        .split_whitespace()
        .filter(|w| w.len() == 64 && w.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_string)
        .collect();
    let mut tasks = Vec::new();
    for id in ids {
        let url = host.url.clone();
        let skip = skip.to_string();
        tasks.push(tokio::spawn(async move {
            let port = url.rsplit(':').next()?.parse().ok()?;
            let name = names(port, &id)?.into_iter().next()?;
            if name == skip {
                return None;
            }
            describe(&url, name).await
        }));
    }
    let mut runs = Vec::new();
    for t in tasks {
        if let Ok(Some(run)) = t.await {
            runs.push(run);
        }
    }
    runs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(runs)
}

/// `argo runs`: print the recorded runs.
pub async fn print(local: &Local) -> Result<(), String> {
    let host = host(local).await?;
    let runs = list(&host, "").await;
    host.release().await;
    let runs = runs?;
    let width = runs.iter().map(|r| r.name.len()).max().unwrap_or(0);
    for run in &runs {
        println!("{}", run.line(width));
    }
    if runs.is_empty() {
        eprintln!("argo: no recorded runs; `argo run <repo>` records one");
    }
    Ok(())
}

/// A database's program and progress; `None` for one that isn't an Argo database.
async fn describe(url: &str, name: String) -> Option<Run> {
    let config = sql(url, &name, "SELECT locator, commit FROM config").await?;
    let row = config.first()?;
    let count = async |table: &str| {
        let rows = sql(url, &name, &format!("SELECT COUNT(*) AS n FROM {table}")).await;
        rows.and_then(|r| r.first()?.first()?.parse().ok())
            .unwrap_or(0)
    };
    let (rust, exchanges) = (count("rust_chunk").await, count("model_exchange").await);
    Some(Run {
        locator: row.first()?.clone(),
        commit: row.get(1).cloned().unwrap_or_default(),
        name,
        rust,
        exchanges,
    })
}

/// A query's rows, parsed from `spacetime sql`'s table.
async fn sql(url: &str, db: &str, query: &str) -> Option<Vec<Vec<String>>> {
    let out = spacetime(&["sql", "--server", url, db, query]).await.ok()?;
    let mut lines = out
        .lines()
        .skip_while(|l| !l.starts_with("---") && !l.contains("-+-"));
    lines.next()?;
    let cell = |c: &str| c.trim().trim_matches('"').to_string();
    Some(
        lines
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split('|').map(cell).collect())
            .collect(),
    )
}

async fn spacetime(args: &[&str]) -> Result<String, String> {
    let out = Command::new("spacetime")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("cannot run spacetime: {e}"))?;
    match out.status.success() {
        true => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        false => Err(format!(
            "spacetime {}: {}",
            args[0],
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// The names a database identity is published under.
fn names(port: u16, id: &str) -> Option<Vec<String>> {
    let body = get(port, &format!("/v1/database/{id}/names"))?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let names = v.get("names")?.as_array()?;
    Some(
        names
            .iter()
            .filter_map(|n| Some(n.as_str()?.to_string()))
            .collect(),
    )
}

fn ping(port: u16) -> bool {
    get(port, "/v1/ping").is_some()
}

/// A plain HTTP/1.0 GET on the local host; the body of a 200 reply.
fn get(port: u16, path: &str) -> Option<String> {
    let addr = ([127, 0, 0, 1], port).into();
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(s, "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n").ok()?;
    let mut reply = String::new();
    s.read_to_string(&mut reply).ok()?;
    let (head, body) = reply.split_once("\r\n\r\n")?;
    head.lines()
        .next()?
        .contains(" 200")
        .then(|| body.to_string())
}

/// `argo-server`'s data directory for this port.
pub fn data_dir(local: &Local) -> PathBuf {
    if let Some(dir) = &local.data_dir {
        return dir.clone();
    }
    let absolute = |v: std::ffi::OsString| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    std::env::var_os("XDG_STATE_HOME")
        .and_then(absolute)
        .or_else(|| Some(absolute(std::env::var_os("HOME")?)?.join(".local/state")))
        .unwrap_or_else(std::env::temp_dir)
        .join("argo-server")
        .join(local.port.to_string())
}
