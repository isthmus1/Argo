//! The command line. Every part runs against one local SpacetimeDB host (`--port`, database
//! `--module`); the flags map one to one onto `argo-server`, `argo-worker` and `argo-ui`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Argo: explode a C program into a live graph and translate it to Rust, all on this machine.
#[derive(Debug, Parser)]
#[command(version, propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    /// Use the part binaries as they are; by default, inside this workspace, `cargo build`
    /// brings them up to date first.
    #[arg(long, global = true)]
    pub no_build: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Serve a repository, translate it with workers and watch it in the GUI.
    Run(RunArgs),
    /// Only serve a repository (SpacetimeDB, ingest and the model proxy).
    Serve(ServeArgs),
    /// Only run workers against an already-running server.
    Work(WorkArgs),
    /// Only open the GUI on an already-running server.
    Ui(UiArgs),
    /// Replay a recorded run (picked from a list without RUN) and watch it in the GUI.
    #[command(visible_alias = "replay")]
    Demo(DemoArgs),
    /// List the recorded runs a demo can replay.
    Runs(Local),
    /// Write a database's translated Rust out as a standalone crate.
    Export(ExportArgs),
}

/// Where the local database is.
#[derive(Debug, Clone, Args)]
pub struct Local {
    /// Port of the local SpacetimeDB host.
    #[arg(long, default_value_t = 3100)]
    pub port: u16,
    /// Database name.
    #[arg(long, short, default_value = "argo")]
    pub module: String,
    /// SpacetimeDB data directory [default: `$XDG_STATE_HOME/argo-server/<port>`].
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
}

impl Local {
    pub fn ws(&self) -> String {
        format!("ws://127.0.0.1:{}", self.port)
    }
}

/// `argo-server`'s options for ingesting a repository.
#[derive(Debug, Clone, Args)]
pub struct ServerOpts {
    /// Address the host and model proxy listen on (`0.0.0.0` for other machines).
    #[arg(long)]
    pub listen: Option<String>,
    /// Model proxy port [default: `--port` + 1].
    #[arg(long)]
    pub proxy_port: Option<u16>,
    /// OpenRouter model every worker uses [default: the server's].
    #[arg(long)]
    pub model: Option<String>,
    /// Pin requests to one OpenRouter provider slug (e.g. `deepinfra`).
    #[arg(long, value_name = "SLUG")]
    pub model_provider: Option<String>,
    /// OpenAI-compatible API the proxy forwards to.
    #[arg(long, value_name = "URL")]
    pub model_url: Option<String>,
    /// Replace a different repository already in the database.
    #[arg(long)]
    pub fresh: bool,
    /// Drop the database when publishing (incompatible schema change only).
    #[arg(long)]
    pub delete_data: bool,
    /// Ingest `HEAD` of a checkout with uncommitted changes, leaving them out.
    #[arg(long)]
    pub allow_dirty: bool,
    /// The `argo-db` module project to publish.
    #[arg(long)]
    pub module_path: Option<PathBuf>,
    /// Preprocessor defines.
    #[arg(short = 'D', value_name = "NAME[=VALUE]")]
    pub defines: Vec<String>,
    /// Preprocessor include directories.
    #[arg(short = 'I', value_name = "DIR")]
    pub includes: Vec<PathBuf>,
}

/// `argo-worker`'s options.
#[derive(Debug, Clone, Args)]
pub struct WorkerOpts {
    /// Workers to run, each translating one chunk at a time.
    #[arg(long, short)]
    pub jobs: Option<u32>,
    /// Rejected submissions per chunk before giving up.
    #[arg(long)]
    pub max_attempts: Option<u32>,
    /// Model turns per chunk before giving up.
    #[arg(long)]
    pub max_turns: Option<u32>,
    /// Translate one chunk per worker, then stop.
    #[arg(long)]
    pub once: bool,
    /// Where the workers clone the repository.
    #[arg(long)]
    pub workdir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// A local directory, or a git URL optionally followed by `#ref` (asked for if left out).
    pub repo: Option<String>,
    #[command(flatten)]
    pub local: Local,
    #[command(flatten)]
    pub server: ServerOpts,
    #[command(flatten)]
    pub worker: WorkerOpts,
    /// Don't start workers.
    #[arg(long)]
    pub no_workers: bool,
    /// Don't open the GUI.
    #[arg(long)]
    pub no_ui: bool,
    /// Ask nothing; use defaults for what isn't given.
    #[arg(long, short)]
    pub yes: bool,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// A local directory, or a git URL optionally followed by `#ref` (asked for if left out).
    pub repo: Option<String>,
    #[command(flatten)]
    pub local: Local,
    #[command(flatten)]
    pub server: ServerOpts,
}

#[derive(Debug, Args)]
pub struct WorkArgs {
    #[command(flatten)]
    pub local: Local,
    #[command(flatten)]
    pub worker: WorkerOpts,
    /// Reach OpenRouter directly with `OPENROUTER_API_KEY` and this model, not the proxy.
    #[arg(long, value_name = "MODEL")]
    pub direct: Option<String>,
    /// The server's model proxy [default: `--port` + 1].
    #[arg(long, value_name = "URL", conflicts_with = "direct")]
    pub proxy: Option<String>,
}

#[derive(Debug, Args)]
pub struct UiArgs {
    #[command(flatten)]
    pub local: Local,
    /// Capture one frame headlessly to this PNG instead of opening a window.
    #[arg(long, value_name = "PNG")]
    pub snap: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct DemoArgs {
    /// The recorded database to replay; picked from a list if left out.
    pub run: Option<String>,
    /// Port of the local SpacetimeDB host.
    #[arg(long, default_value_t = 3100)]
    pub port: u16,
    /// Database the replay plays into.
    #[arg(long, short, default_value = "argo-replay")]
    pub module: String,
    /// SpacetimeDB data directory [default: `$XDG_STATE_HOME/argo-server/<port>`].
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// How many times faster than recorded.
    #[arg(long, short, default_value_t = 100.0)]
    pub speed: f64,
    /// Don't open the GUI.
    #[arg(long)]
    pub no_ui: bool,
}

impl DemoArgs {
    pub fn local(&self) -> Local {
        Local {
            port: self.port,
            module: self.module.clone(),
            data_dir: self.data_dir.clone(),
        }
    }
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    #[command(flatten)]
    pub local: Local,
    /// Directory to write the crate to.
    #[arg(long, short)]
    pub out: PathBuf,
    /// The crate's name.
    #[arg(long)]
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_line_is_consistent() {
        Cli::command().debug_assert();
        let cli = Cli::parse_from(["argo", "run", "r", "-j", "4", "-DX=1", "--fresh"]);
        let Command::Run(a) = cli.command else {
            panic!()
        };
        assert_eq!((a.repo.as_deref(), a.worker.jobs), (Some("r"), Some(4)));
        assert_eq!(
            (a.server.defines, a.server.fresh),
            (vec!["X=1".into()], true)
        );
        assert!(matches!(
            Cli::parse_from(["argo", "demo"]).command,
            Command::Demo(_)
        ));
    }
}
