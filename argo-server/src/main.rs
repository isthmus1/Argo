//! `argo-server`: puts one repository into a SpacetimeDB database that workers and the GUI
//! connect to, from this machine or any other.
//!
//! By default it starts `spacetime start` on `--listen`:PORT (127.0.0.1 unless told otherwise)
//! with its own data directory (a child process, stopped on exit, SIGINT or SIGTERM) and keeps
//! serving it. With `--server URL` it uses a host that is already running instead (another
//! machine's, or `maincloud`), sets the database up there, and exits.
//!
//! Either way it publishes `argo-db`, has the publishing CLI identity
//! `grant_ownership` to its SDK identity, opens the repository with the C front end (in a loader
//! child process, see [`loader`]), records its locator and exact commit, ingests it as `c_chunk`
//! rows through `argo-db-client` (located in the original files through `Opened::places`), then
//! supervises the host until interrupted. Reducers never read files; this process does all of
//! the I/O.
//!
//! A local host also gets the model [`proxy`] on `--proxy-port` (the database's port + 1):
//! workers attach there for a key of their own instead of holding the provider's. The server
//! keeps its owner connection open and [`recorder`] stores every exchange through the proxy in
//! `model_exchange`.
//!
//! `--replay FROM` ingests no repository: it republishes `--module` empty and plays the run
//! recorded in `FROM` into it ([`replay`]), then keeps serving (or, with `--server`, exits).
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
mod endpoint;
mod flags;
mod group;
mod host;
mod ingest;
mod interrupts;
mod loader;
mod proxy;
mod recorder;
mod replay;
mod repo;
#[cfg(test)]
mod scratch;

use std::process::ExitCode;

use clap::Parser;

use args::Args;
use endpoint::Endpoint;
use host::Host;
use interrupts::Interrupts;
use proxy::{Proxy, Upstream};

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
    if let Some(server) = &args.server {
        let outcome = match Endpoint::remote(server) {
            // Nothing to stop afterwards: the host isn't ours.
            Ok(at) => tokio::select! {
                r = setup(&args, &at, async { Ok(()) }) => r.map(|conn| conn.close()),
                why = interrupts.recv() => Err(format!("{why}; stopped")),
            },
            Err(e) => Err(e),
        };
        return match outcome {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fail(&e),
        };
    }
    let data_dir = args.data_dir();
    // A replay asks no model, and its exchanges are already recorded.
    let proxy = match args.replay.is_some() {
        true => Ok(None),
        false => proxy(&args, &data_dir),
    };
    let proxy = match proxy {
        Ok(proxy) => proxy,
        Err(e) => return fail(&e),
    };
    let host = match Host::spawn(args.listen, args.port, &data_dir) {
        Ok(host) => host,
        Err(e) => return fail(&e),
    };
    eprintln!("argo-server: host data in {}", data_dir.display());
    // Every step races the host dying and a stop signal; losing the race drops the step, which
    // kills whatever child process it was waiting on.
    let outcome = tokio::select! {
        r = serve(&args, &host, proxy) => r,
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

/// The model proxy, bound, when this machine has the provider's endpoint and key to give it.
fn proxy(args: &Args, data_dir: &std::path::Path) -> Result<Option<Proxy>, String> {
    let upstream = Upstream::from_env(&args.model_url, &args.model, args.provider());
    let upstream = match upstream {
        Ok(upstream) => upstream,
        Err(why) => {
            eprintln!(
                "argo-server: model proxy off ({why}); workers need --direct and a key of their own"
            );
            return Ok(None);
        }
    };
    let port = args.proxy_port()?;
    let log = data_dir.join("usage.log");
    let proxy = Proxy::bind(std::net::SocketAddr::new(args.listen, port), upstream, &log)?;
    eprintln!(
        "argo-server: model proxy on {} for workers, forwarding to {}; usage in {}",
        endpoint::advertised("http", args.listen, port),
        proxy.upstream(),
        log.display()
    );
    Ok(Some(proxy))
}

/// Serve the model proxy and bring the host up while the repository loads, ingest, then keep
/// serving. Never returns `Ok`: a running server ends by being interrupted or by its host exiting.
async fn serve(args: &Args, host: &Host, proxy: Option<Proxy>) -> Result<(), String> {
    let exchanges = proxy.as_ref().map(Proxy::record);
    let database = async {
        let conn = setup(args, &Endpoint::local(args.port), host.ready()).await?;
        eprintln!(
            "argo-server: serving {} database {}; Ctrl-C to stop",
            endpoint::advertised("ws", args.listen, args.port),
            args.module
        );
        match exchanges {
            Some(rx) => recorder::record(&conn, rx).await,
            None => std::future::pending::<Result<(), String>>().await,
        }
    };
    let proxy = async {
        match proxy {
            Some(proxy) => proxy.serve().await,
            None => std::future::pending().await,
        }
    };
    tokio::try_join!(database, proxy).map(|_| ())
}

/// Publish the module to `at` once `ready`, loading the repository meanwhile, then take
/// ownership and ingest it (or, with `--replay`, play the recording in). Returns the owner's
/// connection, which records the proxy's exchanges.
async fn setup(
    args: &Args,
    at: &Endpoint,
    ready: impl Future<Output = Result<(), String>>,
) -> Result<conn::Conn, String> {
    if let Some(from) = &args.replay {
        return setup_replay(args, at, ready, from).await;
    }
    let up = async {
        ready.await?;
        cli::publish(&at.cli, &args.module_path, &args.module, args.delete_data).await
    };
    let (loaded, ()) = tokio::try_join!(loader::load(), up)?;

    let (conn, me) = conn::connect(&at.ws, &args.module).await?;
    let outcome = async {
        cli::grant(&at.cli, &args.module, me).await?;
        ingest::ingest(&conn, &loaded, args.fresh).await
    }
    .await;
    let sent = match outcome {
        Ok(sent) => sent,
        Err(e) => {
            conn.close();
            return Err(e);
        }
    };
    ingest::report(&loaded, &sent);
    eprintln!(
        "argo-server: workers and the GUI connect to {} database {}",
        at.ws, args.module
    );
    owner_hint(args, at);
    Ok(conn)
}

/// `--replay FROM`: a fresh `--module` on the same host, then the recording played into it.
async fn setup_replay(
    args: &Args,
    at: &Endpoint,
    ready: impl Future<Output = Result<(), String>>,
    from: &str,
) -> Result<conn::Conn, String> {
    if *from == args.module {
        return Err(format!(
            "--replay {from} would replay into itself; pick another --module"
        ));
    }
    ready.await?;
    // Every replay starts from nothing: this module's earlier replay is dropped.
    cli::publish(&at.cli, &args.module_path, &args.module, true).await?;
    let (conn, me) = conn::connect(&at.ws, &args.module).await?;
    let (source, _) = match conn::connect(&at.ws, from).await {
        Ok(source) => source,
        Err(e) => {
            conn.close();
            return Err(format!("the recording {from}: {e}"));
        }
    };
    eprintln!(
        "argo-server: the GUI watches the replay at {} database {}",
        at.ws, args.module
    );
    let outcome = async {
        cli::grant(&at.cli, &args.module, me).await?;
        replay::replay(&conn, &source, from, args.speed()).await
    }
    .await;
    source.close();
    match outcome {
        Ok(()) => Ok(conn),
        Err(e) => {
            conn.close();
            Err(e)
        }
    }
}

/// The owner is this run's anonymous connection, gone when the server stops; say how to take
/// ownership for `requeue` and the like.
fn owner_hint(args: &Args, at: &Endpoint) {
    eprintln!(
        "argo-server: for requeue, first `spacetime call --server {} {} grant_ownership \
         '{{\"__identity__\": \"0x<your CLI identity>\"}}'`",
        at.cli, args.module
    );
}

fn fail(e: &str) -> ExitCode {
    eprintln!("argo-server: {e}");
    ExitCode::FAILURE
}
