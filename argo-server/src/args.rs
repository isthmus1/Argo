//! The command line: which repository to serve and how to host it.

use std::path::{Path, PathBuf};

use argo_c::project::git::GitRef;
use clap::Parser;

/// The OpenRouter model workers use unless `--model` says otherwise.
const MODEL: &str = "nvidia/nemotron-3.5-lightning";
/// The provider pinned unless `--model-provider` says otherwise. The worker's agent needs tool
/// calling, which OpenRouter lists Fireworks' endpoint for this model as lacking.
const PROVIDER: &str = "deepinfra";
/// OpenRouter's OpenAI-compatible API, the proxy's default upstream; `argo-worker --direct`
/// sends to the same.
const OPENROUTER: &str = "https://openrouter.ai/api";

/// Where `argo-db` lives in this checkout; the default module project to build and publish.
const MODULE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../argo-db");

/// Serve one C repository through SpacetimeDB for workers and the GUI.
#[derive(Debug, Parser)]
#[command(version)]
pub struct Args {
    /// A local directory, or a git URL or path optionally followed by `#ref`.
    #[arg(required_unless_present = "replay")]
    pub repo: Option<String>,
    /// Replay the run recorded in database `FROM` on the same host into `--module` instead of
    /// ingesting a repository: its program, then every claim, result and model exchange
    /// (`model_exchange`) at `--speed`. `--module` is dropped and republished first.
    #[arg(long, value_name = "FROM", conflicts_with_all = ["repo", "fresh", "allow_dirty", "proxy_port", "delete_data"])]
    pub replay: Option<String>,
    /// How many times faster than recorded a `--replay` plays [default: 100]. It conflicts with a
    /// repository too: clap drops `requires` when the required flag conflicts with one present.
    #[arg(long, requires = "replay", conflicts_with = "repo", value_parser = speed)]
    pub speed: Option<f64>,
    /// Use an already-running SpacetimeDB instead of starting one: `http://host:port`,
    /// `https://host`, or `maincloud` (SpacetimeDB's cloud; `spacetime login` first). The module
    /// is published there and the repository ingested, then `argo-server` exits; the database
    /// keeps serving workers and the GUI. Publish and grant use this machine's `spacetime` CLI
    /// identity, which must be the database's publisher.
    #[arg(long, value_name = "URL")]
    pub server: Option<String>,
    /// Port for the local SpacetimeDB host. Not 3000, which a system host may already use.
    #[arg(long, default_value_t = 3100)]
    pub port: u16,
    /// Address the local host listens on. `0.0.0.0` accepts workers and the GUI from other
    /// machines; put it behind a TLS proxy before exposing it to a network you don't trust.
    #[arg(long, default_value = "127.0.0.1", conflicts_with = "server")]
    pub listen: std::net::IpAddr,
    /// Port for the model proxy on the `--listen` address (default: `--port` + 1, where workers
    /// look for it). Workers attach there for a key of their own; it forwards their completions to
    /// `OPENAI_BASE_URL` with this machine's `OPENAI_API_KEY`, so workers hold neither, and logs
    /// usage per worker to `usage.log` in the data dir. Off when `OPENROUTER_API_KEY` is unset.
    #[arg(long, conflicts_with = "server")]
    pub proxy_port: Option<u16>,
    /// The OpenRouter model every worker uses: workers are told it when they attach, and the
    /// proxy sends every request as it.
    #[arg(long, default_value = MODEL, value_parser = model_name, conflicts_with = "server")]
    pub model: String,
    /// Pin every request to this OpenRouter provider (a slug), with no fallback to others;
    /// `any` lets OpenRouter pick.
    #[arg(long, value_name = "SLUG|any", default_value = PROVIDER, conflicts_with = "server")]
    pub model_provider: String,
    /// The OpenAI-compatible API the proxy forwards to with `OPENROUTER_API_KEY`.
    #[arg(long, value_name = "URL", default_value = OPENROUTER, conflicts_with = "server")]
    pub model_url: String,
    /// SpacetimeDB data directory, kept across runs so a restart resumes instead of re-ingesting;
    /// it also holds a full checkout of the repository, which is parsed. Must be outside the
    /// repository (default: `$XDG_STATE_HOME/argo-server/<port>`, `~/.local/state/...` without
    /// it).
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Database name to publish the module under.
    #[arg(long, default_value = "argo")]
    pub module: String,
    /// Reset the database (`ingest_reset`: all chunks and Rust results) before ingesting: the
    /// explicit reset a different commit needs.
    #[arg(long)]
    pub fresh: bool,
    /// Drop the database when publishing; only for a module whose schema changed incompatibly.
    #[arg(long)]
    pub delete_data: bool,
    /// Ingest `HEAD` of a checkout with uncommitted changes, leaving the changes out.
    #[arg(long)]
    pub allow_dirty: bool,
    /// The `argo-db` module project to build and publish.
    #[arg(long, default_value = MODULE_PATH)]
    pub module_path: PathBuf,
    /// Preprocessor defines, `-DNAME` or `-DNAME=value`.
    #[arg(short = 'D', value_name = "NAME[=VALUE]")]
    pub defines: Vec<String>,
    /// Preprocessor include directories.
    #[arg(short = 'I', value_name = "DIR")]
    pub includes: Vec<PathBuf>,
    /// Run as the loader child process (see `loader`).
    #[arg(long, hide = true)]
    pub loader: bool,
}

/// Where the repository comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repo {
    /// An existing directory, opened in place.
    Dir(PathBuf),
    /// Anything else: fetched (a URL, or a path with `#ref`) into the checkout under the data dir.
    Git(GitRef),
}

/// Workers record the model with each result, and the module rejects a longer name.
fn model_name(name: &str) -> Result<String, String> {
    let max = argo_db_client::MAX_MODEL_BYTES;
    match name.len() {
        n if (1..=max).contains(&n) => Ok(name.to_string()),
        n => Err(format!("model names are 1 to {max} bytes, not {n}")),
    }
}

/// A replay speed: a finite factor above zero.
fn speed(text: &str) -> Result<f64, String> {
    match text.parse::<f64>() {
        Ok(s) if s.is_finite() && s > 0.0 => Ok(s),
        _ => Err(format!("a speed is a number above 0, not {text:?}")),
    }
}

impl Args {
    /// `--speed`, or the replay default.
    pub fn speed(&self) -> f64 {
        self.speed.unwrap_or(100.0)
    }

    /// The repository to ingest; only `--replay` runs have none, and they never ask.
    pub fn repo(&self) -> Repo {
        let repo = self
            .repo
            .as_deref()
            .expect("clap requires a repository without --replay");
        if Path::new(repo).is_dir() {
            Repo::Dir(PathBuf::from(repo))
        } else {
            Repo::Git(GitRef::parse(repo))
        }
    }

    /// Extra preprocessor flags, in the form `argo::project::Options` takes.
    pub fn flags(&self) -> Vec<String> {
        let defines = self.defines.iter().map(|d| format!("-D{d}"));
        let includes = self.includes.iter().map(|i| format!("-I{}", i.display()));
        defines.chain(includes).collect()
    }

    /// A persistent per-user state directory by default: the system temp dir is often a
    /// RAM-backed tmpfs, too small for a large repository's checkout and lost on reboot.
    pub fn data_dir(&self) -> PathBuf {
        self.data_dir
            .clone()
            .unwrap_or_else(|| state_home().join("argo-server").join(self.port.to_string()))
    }

    /// The provider to pin, `None` for OpenRouter's choice (`--model-provider any`).
    pub fn provider(&self) -> Option<&str> {
        Some(self.model_provider.as_str()).filter(|p| *p != "any")
    }

    /// The model proxy's port: `--proxy-port`, else the database's port + 1.
    pub fn proxy_port(&self) -> Result<u16, String> {
        self.proxy_port
            .or_else(|| self.port.checked_add(1))
            .ok_or_else(|| format!("--port {}: choose a --proxy-port", self.port))
    }

    /// Where a commit is checked out to be parsed, reused across runs.
    pub fn checkout_dir(&self) -> PathBuf {
        self.data_dir().join("checkout")
    }

    /// Refuse a data dir inside a directory repository: the host's files would make the tree look
    /// dirty, and a tree read in place would include the checkout's copy of itself.
    pub fn check_data_dir(&self) -> Result<(), String> {
        if self.replay.is_some() {
            return Ok(());
        }
        let Repo::Dir(repo) = self.repo() else {
            return Ok(());
        };
        let data = resolved(&self.data_dir());
        let repo = std::fs::canonicalize(&repo).map_err(|e| format!("{}: {e}", repo.display()))?;
        if data.starts_with(&repo) {
            return Err(format!(
                "the data dir {} is inside the repository {}; choose one outside it",
                data.display(),
                repo.display()
            ));
        }
        Ok(())
    }
}

/// `$XDG_STATE_HOME` (the XDG spec ignores a relative one), else `$HOME/.local/state`, else the
/// system temp dir.
fn state_home() -> PathBuf {
    let absolute = |v: std::ffi::OsString| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    std::env::var_os("XDG_STATE_HOME")
        .and_then(absolute)
        .or_else(|| Some(absolute(std::env::var_os("HOME")?)?.join(".local/state")))
        .unwrap_or_else(std::env::temp_dir)
}

/// `path` with its longest existing ancestor canonicalized, so a data dir that doesn't exist yet
/// still compares correctly (through symlinks and `..`).
fn resolved(path: &Path) -> PathBuf {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.into());
    abs.ancestors()
        .find_map(|a| {
            Some(
                std::fs::canonicalize(a)
                    .ok()?
                    .join(abs.strip_prefix(a).ok()?),
            )
        })
        .unwrap_or(abs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from(std::iter::once("argo-server").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn defaults() {
        let a = parse(&["https://example.com/r.git"]);
        assert_eq!((a.port, a.module.as_str(), a.fresh), (3100, "argo", false));
        assert_eq!(
            (a.server.as_deref(), a.listen),
            (None, "127.0.0.1".parse().unwrap())
        );
        assert!(!a.delete_data && !a.allow_dirty);
        assert!(
            a.module_path.join("Cargo.toml").is_file(),
            "argo-db in this checkout"
        );
        assert_eq!(a.data_dir(), state_home().join("argo-server/3100"));
        assert!(state_home().is_absolute());
        assert!(a.flags().is_empty());
        assert!(!a.loader, "hidden");
        assert_eq!(a.checkout_dir(), a.data_dir().join("checkout"));
        assert_eq!(a.proxy_port(), Ok(3101));
        assert_eq!(a.model, "nvidia/nemotron-3.5-lightning");
        assert_eq!(
            (a.provider(), a.model_url.as_str()),
            (Some("deepinfra"), "https://openrouter.ai/api")
        );
        assert_eq!(parse(&["r", "--model-provider", "any"]).provider(), None);
        let demo = parse(&[
            "r",
            "--model",
            "nvidia/nemotron-3.5-lightning",
            "--model-provider",
            "fireworks",
        ]);
        assert_eq!(demo.provider(), Some("fireworks"));
        assert!(Args::try_parse_from(["argo-server", "r", "--model", ""]).is_err());
        assert!(
            parse(&["r", "--server", "maincloud"]).provider() == Some("deepinfra"),
            "defaults don't conflict"
        );
        let p = parse(&["r", "--port", "4000", "--proxy-port", "5000"]);
        assert_eq!(p.proxy_port(), Ok(5000));
        assert!(parse(&["r", "--port", "65535"]).proxy_port().is_err());
        let remote = [
            "argo-server",
            "r",
            "--server",
            "maincloud",
            "--proxy-port",
            "1",
        ];
        assert!(
            Args::try_parse_from(remote).is_err(),
            "no proxy without a local host"
        );
    }

    #[test]
    fn the_data_dir_must_be_outside_a_directory_repository() {
        let dir = env!("CARGO_MANIFEST_DIR");
        let inside = format!("{dir}/src/not-yet");
        assert!(
            parse(&[dir, "--data-dir", &inside])
                .check_data_dir()
                .is_err()
        );
        assert!(parse(&[dir, "--data-dir", dir]).check_data_dir().is_err());
        let outside = format!("{dir}/../target/argo-server-data");
        assert!(
            parse(&[dir, "--data-dir", &outside])
                .check_data_dir()
                .is_ok()
        );
        let url = "https://example.com/r.git";
        assert!(parse(&[url, "--data-dir", "."]).check_data_dir().is_ok());
    }

    #[test]
    fn options_and_preprocessor_flags() {
        let a = parse(&[
            "repo",
            "--port",
            "3102",
            "--data-dir",
            "/tmp/d",
            "--module",
            "m",
            "--fresh",
            "--delete-data",
            "--allow-dirty",
            "-DFOO",
            "-D",
            "BAR=2",
            "-Iinc",
            "-I",
            "/abs",
        ]);
        assert_eq!((a.port, a.module.as_str(), a.fresh), (3102, "m", true));
        assert!(a.delete_data && a.allow_dirty);
        assert_eq!(a.data_dir(), PathBuf::from("/tmp/d"));
        assert_eq!(a.flags(), ["-DFOO", "-DBAR=2", "-Iinc", "-I/abs"]);
    }

    #[test]
    fn a_server_or_a_listen_address_but_not_both() {
        let a = parse(&["repo", "--server", "maincloud", "--module", "mine"]);
        assert_eq!(a.server.as_deref(), Some("maincloud"));
        assert_eq!(
            parse(&["repo", "--listen", "0.0.0.0"]).listen.to_string(),
            "0.0.0.0"
        );
        let both = [
            "argo-server",
            "repo",
            "--server",
            "maincloud",
            "--listen",
            "0.0.0.0",
        ];
        assert!(Args::try_parse_from(both).is_err());
    }

    #[test]
    fn a_replay_needs_no_repository_and_takes_a_speed() {
        let a = parse(&["--replay", "argo", "--module", "argo-replay"]);
        assert_eq!(
            (a.replay.as_deref(), a.speed(), a.repo.as_deref()),
            (Some("argo"), 100.0, None)
        );
        assert!(a.check_data_dir().is_ok());
        assert_eq!(parse(&["--replay", "argo", "--speed", "2.5"]).speed(), 2.5);
        let no = |extra: &[&str]| {
            let args = ["argo-server"].iter().chain(extra);
            Args::try_parse_from(args).is_err()
        };
        assert!(no(&[]), "a repository or a replay");
        assert!(no(&["r", "--replay", "argo"]), "not both");
        assert!(no(&["r", "--speed", "2"]), "a speed only for a replay");
        assert!(no(&["--replay", "argo", "--speed", "0"]));
        assert!(no(&["--replay", "argo", "--speed", "nan"]));
    }

    #[test]
    fn directories_open_in_place_and_the_rest_is_cloned() {
        let dir = env!("CARGO_MANIFEST_DIR");
        assert_eq!(parse(&[dir]).repo(), Repo::Dir(dir.into()));
        let at_ref = format!("{dir}#main");
        assert_eq!(
            parse(&[&at_ref]).repo(),
            Repo::Git(GitRef::new(dir, Some("main")))
        );
        assert_eq!(
            parse(&["https://example.com/r.git#v1"]).repo(),
            Repo::Git(GitRef::new("https://example.com/r.git", Some("v1")))
        );
    }
}
