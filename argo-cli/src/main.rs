//! `argo`: one command line for everything on one machine.
//!
//! It launches the existing binaries (`argo-server`, `argo-worker`, `argo-ui`, `argo-export`) as
//! child processes beside its own executable, wires them to the same local database, starts the
//! workers and the GUI once the server says the database is ready, and stops them all together
//! (Ctrl-C, or any one of them ending). Missing arguments are asked for with small prompts when
//! a terminal is attached.
//!
//! - `argo run <repo>`: server, workers and GUI.
//! - `argo serve <repo>`, `argo work`, `argo ui`: one part each.
//! - `argo demo [RUN]`: pick a recorded run (arrows, type to filter, Enter) and replay it.
//! - `argo runs`: list the recorded runs. `argo export --out DIR`: write the Rust out as a crate.

mod args;
mod cmds;
mod procs;
mod runs;
mod wizard;

use std::process::ExitCode;

use clap::Parser;

use args::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return fail(&format!("cannot start the async runtime: {e}")),
    };
    match rt.block_on(run(cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    let bins = procs::Bins::locate(cli.no_build)?;
    match cli.command {
        Command::Run(a) => cmds::run(&bins, a).await,
        Command::Serve(a) => cmds::serve(&bins, a).await,
        Command::Work(a) => cmds::work(&bins, a).await,
        Command::Ui(a) => cmds::ui(&bins, a).await,
        Command::Demo(a) => cmds::demo(&bins, a).await,
        Command::Runs(a) => runs::print(&a).await,
        Command::Export(a) => cmds::export(&bins, a).await,
    }
}

fn fail(e: &str) -> ExitCode {
    eprintln!("argo: {e}");
    ExitCode::FAILURE
}
