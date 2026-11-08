//! Each subcommand: the part commands it builds and how it supervises them.

use tokio::process::Command;

use crate::args::{
    DemoArgs, ExportArgs, Local, RunArgs, ServeArgs, ServerOpts, UiArgs, WorkArgs, WorkerOpts,
};
use crate::procs::{Bins, Group, Role};
use crate::{runs, wizard};

/// `argo-server` lines that mean the database is ready for workers and the GUI.
const INGESTED: &[&str] = &["workers and the GUI connect to"];
const REPLAYING: &[&str] = &["the GUI watches the replay"];

pub async fn run(bins: &Bins, mut a: RunArgs) -> Result<(), String> {
    let ask = !a.yes && wizard::interactive();
    let repo = repo(a.repo.take(), ask)?;
    if !a.no_workers && std::env::var_os("OPENROUTER_API_KEY").is_none() {
        eprintln!("argo: OPENROUTER_API_KEY is unset, so the server has no model for workers");
        if ask && !wizard::confirm("Continue without workers?", true)? {
            return Err("set OPENROUTER_API_KEY and run again".into());
        }
        a.no_workers = true;
    }
    if !a.no_workers && a.worker.jobs.is_none() && ask {
        a.worker.jobs = Some(wizard::jobs()?);
    }
    let mut g = Group::new();
    let cmd = server(bins, &a.local, &a.server, &repo);
    let ready = g.spawn_watched("argo-server", cmd, Role::Essential, INGESTED)?;
    if let Err(stop) = g.until(ready).await {
        return g.shutdown(stop).await;
    }
    if !a.no_workers {
        g.spawn("argo-worker", worker(bins, &a.local, &a.worker), Role::Free)?;
    }
    if !a.no_ui {
        g.spawn("argo-ui", ui_cmd(bins, &a.local), Role::Essential)?;
    }
    g.supervise().await
}

pub async fn serve(bins: &Bins, a: ServeArgs) -> Result<(), String> {
    let repo = repo(a.repo, wizard::interactive())?;
    let mut g = Group::new();
    g.spawn(
        "argo-server",
        server(bins, &a.local, &a.server, &repo),
        Role::Essential,
    )?;
    g.supervise().await
}

pub async fn work(bins: &Bins, a: WorkArgs) -> Result<(), String> {
    let mut cmd = worker(bins, &a.local, &a.worker);
    if let Some(model) = &a.direct {
        cmd.args(["--direct", "--model", model]);
    }
    if let Some(url) = &a.proxy {
        cmd.args(["--proxy", url]);
    }
    let mut g = Group::new();
    g.spawn("argo-worker", cmd, Role::Essential)?;
    g.supervise().await
}

pub async fn ui(bins: &Bins, a: UiArgs) -> Result<(), String> {
    let mut cmd = ui_cmd(bins, &a.local);
    if let Some(png) = &a.snap {
        cmd.arg("--snap").arg(png);
    }
    let mut g = Group::new();
    g.spawn("argo-ui", cmd, Role::Essential)?;
    g.supervise().await
}

/// Pick a recorded run (unless named), then replay it and watch it.
pub async fn demo(bins: &Bins, a: DemoArgs) -> Result<(), String> {
    let local = a.local();
    let host = runs::host(&local).await?;
    let from = match &a.run {
        Some(run) => run.clone(),
        None => match pick(&host, &a.module).await {
            Ok(run) => run,
            Err(e) => {
                host.release().await;
                return Err(e);
            }
        },
    };
    // A host this started only to list the runs makes way for the server's own; one that was
    // already running is replayed into, and the server exits when the replay ends.
    let remote = (!host.ours()).then(|| host.url.clone());
    host.release().await;
    eprintln!("argo: replaying {from} into {} at {}x", a.module, a.speed);
    let mut cmd = bins.cmd("argo-server");
    cmd.args([
        "--replay",
        &from,
        "--module",
        &a.module,
        "--speed",
        &a.speed.to_string(),
    ]);
    let role = match &remote {
        Some(url) => {
            cmd.args(["--server", url]);
            Role::Finishes
        }
        None => {
            cmd.args(["--port", &a.port.to_string()]);
            if let Some(dir) = &a.data_dir {
                cmd.arg("--data-dir").arg(dir);
            }
            Role::Essential
        }
    };
    let mut g = Group::new();
    let ready = g.spawn_watched("argo-server", cmd, role, REPLAYING)?;
    if let Err(stop) = g.until(ready).await {
        return g.shutdown(stop).await;
    }
    if !a.no_ui {
        g.spawn("argo-ui", ui_cmd(bins, &local), Role::Essential)?;
    }
    g.supervise().await
}

async fn pick(host: &runs::Host, skip: &str) -> Result<String, String> {
    let mut runs = runs::list(host, skip).await?;
    match runs.len() {
        0 => Err("no recorded runs; `argo run <repo>` records one".into()),
        1 if !wizard::interactive() => Ok(runs.remove(0).name),
        _ if !wizard::interactive() => Err("several recorded runs; name one: argo demo RUN".into()),
        _ => Ok(runs.remove(wizard::pick(&runs)?).name),
    }
}

pub async fn export(bins: &Bins, a: ExportArgs) -> Result<(), String> {
    let mut cmd = bins.cmd("argo-export");
    cmd.args(["--db", &a.local.ws(), "--module", &a.local.module]);
    cmd.arg("--out").arg(&a.out);
    if let Some(name) = &a.name {
        cmd.args(["--name", name]);
    }
    let mut g = Group::new();
    g.spawn("argo-export", cmd, Role::Essential)?;
    g.supervise().await
}

fn repo(given: Option<String>, ask: bool) -> Result<String, String> {
    match given {
        Some(repo) => Ok(repo),
        None if ask => wizard::repo(),
        None => Err("name a repository: a directory, or a git URL[#ref]".into()),
    }
}

fn server(bins: &Bins, local: &Local, o: &ServerOpts, repo: &str) -> Command {
    let mut cmd = bins.cmd("argo-server");
    cmd.arg(repo)
        .args(["--port", &local.port.to_string(), "--module", &local.module]);
    let opt = |cmd: &mut Command, flag: &str, v: Option<String>| {
        if let Some(v) = v {
            cmd.args([flag, &v]);
        }
    };
    if let Some(dir) = &local.data_dir {
        cmd.arg("--data-dir").arg(dir);
    }
    opt(&mut cmd, "--listen", o.listen.clone());
    opt(
        &mut cmd,
        "--proxy-port",
        o.proxy_port.map(|p| p.to_string()),
    );
    opt(&mut cmd, "--model", o.model.clone());
    opt(&mut cmd, "--model-provider", o.model_provider.clone());
    opt(&mut cmd, "--model-url", o.model_url.clone());
    if let Some(path) = &o.module_path {
        cmd.arg("--module-path").arg(path);
    }
    for (on, flag) in [
        (o.fresh, "--fresh"),
        (o.delete_data, "--delete-data"),
        (o.allow_dirty, "--allow-dirty"),
    ] {
        if on {
            cmd.arg(flag);
        }
    }
    for d in &o.defines {
        cmd.arg(format!("-D{d}"));
    }
    for i in &o.includes {
        cmd.arg("-I").arg(i);
    }
    cmd
}

fn worker(bins: &Bins, local: &Local, o: &WorkerOpts) -> Command {
    let mut cmd = bins.cmd("argo-worker");
    cmd.args(["--db", &local.ws(), "--module", &local.module]);
    for (flag, v) in [
        ("--jobs", o.jobs),
        ("--max-attempts", o.max_attempts),
        ("--max-turns", o.max_turns),
    ] {
        if let Some(v) = v {
            cmd.args([flag, &v.to_string()]);
        }
    }
    if o.once {
        cmd.arg("--once");
    }
    if let Some(dir) = &o.workdir {
        cmd.arg("--workdir").arg(dir);
    }
    if let Some(test) = &o.test_cmd {
        cmd.args(["--test-cmd", test]);
    }
    cmd
}

fn ui_cmd(bins: &Bins, local: &Local) -> Command {
    let mut cmd = bins.cmd("argo-ui");
    cmd.args(["--db", &local.ws(), "--module", &local.module]);
    cmd
}
