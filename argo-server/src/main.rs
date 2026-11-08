//! `argo-server`: owns a local SpacetimeDB for one repository.
//!
//! Starts `spacetime start` on 127.0.0.1:PORT with its own data directory (a child process,
//! stopped on exit, SIGINT or SIGTERM), publishes `argo-db`, has the publishing CLI identity
//! `grant_ownership` to its SDK identity, opens the repository with the C front end (in a loader
//! child process, see [`loader`]), records its locator and exact commit, ingests it as `c_chunk`
//! rows through `argo-db-client` (located in the original files through `Opened::places`), then
//! supervises the host until interrupted. Reducers never read files; this process does all of
//! the I/O.
//!
//! Unix-only: children run in their own process groups and are stopped by signal ([`group`]),
//! and SIGINT/SIGTERM are handled through `tokio::signal::unix`. Linux additionally kills them
//! if `argo-server` itself is SIGKILLed; other Unixes don't (see [`group::isolate`]).

#[cfg(not(unix))]
compile_error!(
    "argo-server is Unix-only: it manages its child processes with process groups and signals"
);

mod args;
mod checkout;
mod cli;
mod commit;
mod conn;
mod flags;
mod group;
mod host;
mod ingest;
mod interrupts;
mod loader;
mod repo;
#[cfg(test)]
mod scratch;

use std::process::ExitCode;

use clap::Parser;

use args::Args;
use host::Host;
use interrupts::Interrupts;

fn main() -> ExitCode {
    let args = Args::parse();
    if args.loader {
        return loader::child(&args);
    }
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return fail(&format!("cannot start the async runtime: {e}")),
    };
    let code = rt.block_on(run(args));
    // An abandoned SDK connection may still have tasks; nothing they do matters any more.
    rt.shutdown_background();
    code
}

async fn run(args: Args) -> ExitCode {
    let mut interrupts = match Interrupts::install() {
        Ok(i) => i,
        Err(e) => return fail(&format!("cannot install signal handlers: {e}")),
    };
    if let Err(e) = args.check_data_dir() {
        return fail(&e);
    }
    let data_dir = args.data_dir();
    let host = match Host::spawn(args.port, &data_dir) {
        Ok(host) => host,
        Err(e) => return fail(&e),
    };
    eprintln!("argo-server: host data in {}", data_dir.display());
    // Every step races the host dying and a stop signal; losing the race drops the step, which
    // kills whatever child process it was waiting on.
    let outcome = tokio::select! {
        r = serve(&args, &host) => r,
        e = host.exited() => Err(e),
        why = interrupts.recv() => {
            eprintln!("argo-server: {why}; stopping");
            Ok(())
        }
    };
    host.stop(&mut interrupts).await;
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

/// Bring the host up while the repository loads, ingest, then keep serving. Never returns
/// `Ok`: a running server ends by being interrupted or by its host exiting.
async fn serve(args: &Args, host: &Host) -> Result<(), String> {
    let server = format!("http://127.0.0.1:{}", args.port);
    let up = async {
        host.ready().await?;
        cli::publish(&server, &args.module_path, &args.module, args.delete_data).await
    };
    let (loaded, ()) = tokio::try_join!(loader::load(), up)?;

    let uri = format!("ws://127.0.0.1:{}", args.port);
    let (conn, me) = conn::connect(&uri, &args.module).await?;
    let outcome = async {
        cli::grant(&server, &args.module, me).await?;
        ingest::ingest(&conn, &loaded, args.fresh).await
    }
    .await;
    // The server has nothing more to say to the database; it only keeps the host alive.
    conn.close();
    ingest::report(&loaded, &outcome?);
    eprintln!(
        "argo-server: serving {uri} database {}; Ctrl-C to stop",
        args.module
    );
    // The owner identity was this run's anonymous connection, now closed; say how to get it back.
    eprintln!(
        "argo-server: for requeue, first `spacetime call --server {server} {} grant_ownership \
         '{{\"__identity__\": \"0x<your CLI identity>\"}}'`",
        args.module
    );
    std::future::pending().await
}

fn fail(e: &str) -> ExitCode {
    eprintln!("argo-server: {e}");
    ExitCode::FAILURE
}
