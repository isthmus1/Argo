use std::io::Write;
use std::path::PathBuf;

use main::{Parser, project, report, templates};

pub(crate) struct Args {
    path: PathBuf,
    graph_json: bool,
    protocol_json: bool,
    /// Preprocessor flags (`-D`, `-U`, `-I`) for project inputs.
    flags: Vec<String>,
}

impl Args {
    pub fn parse() -> Result<Self, String> {
        let (mut path, mut graph_json, mut positional) = (None, false, false);
        let mut protocol_json = false;
        let mut flags = vec![];
        for arg in std::env::args_os().skip(1) {
            if !positional && arg == "--" {
                positional = true;
            } else if !positional && arg == "--graph-json" {
                graph_json = true;
            } else if !positional && arg == "--protocol-json" {
                protocol_json = true;
            } else if !positional
                && let Some(flag) = arg
                    .to_str()
                    .filter(|a| ["-D", "-U", "-I"].iter().any(|p| a.starts_with(p)))
            {
                flags.push(flag.to_string());
            } else if !positional && arg.to_str().is_some_and(|arg| arg.starts_with('-')) {
                return Err(format!("unknown option {}", arg.to_string_lossy()));
            } else if path.replace(PathBuf::from(arg)).is_some() {
                return Err("expected at most one C input path".into());
            }
        }
        Ok(Self {
            path: path.unwrap_or_else(|| "tests/fixture.c".into()),
            graph_json,
            protocol_json,
            flags,
        })
    }
}

pub(crate) fn run(args: Args) -> Result<(), String> {
    // A directory or git reference is a whole project: preprocessed and combined into one graph.
    let spec = args.path.to_string_lossy();
    let remote = ["https://", "http://", "git@", "ssh://", "git://", "file://"]
        .iter()
        .any(|p| spec.starts_with(p));
    if args.protocol_json && (remote || args.path.is_dir()) {
        return project(&args, remote);
    }
    let parser = Parser::default();
    let parsed = parser.parse_file(&args.path)?;
    if !args.graph_json && !args.protocol_json {
        report::header(&args.path, &parser, &parsed);
    }
    report::diagnostics(&parsed);
    if parsed.failed() {
        return Err(format!(
            "{} reported errors, no AST emitted",
            args.path.display()
        ));
    }
    if args.protocol_json {
        return protocol(&args, &parsed);
    }
    if args.graph_json {
        let graph = parsed
            .ast
            .build_graph(&parsed.source, templates::all())
            .map_err(|error| error.to_string())?;
        let json = graph.to_json().map_err(|error| error.to_string())?;
        return writeln!(std::io::stdout().lock(), "{json}").map_err(|error| error.to_string());
    }
    println!("\n=== AST ===");
    for node in &parsed.ast.roots {
        report::tree(node, 0);
    }
    let matches = parsed.ast.match_templates(templates::all());
    report::template_summary(&matches);
    println!("\n=== rucc_ast::print: the tree written back out as C ===");
    print!("{}", parsed.as_c());
    Ok(())
}

/// Emit a whole project's frozen snapshot; the per-file outcome goes to stderr.
fn project(args: &Args, remote: bool) -> Result<(), String> {
    let opts = project::Options {
        flags: args.flags.clone(),
        ..Default::default()
    };
    let log = |step: project::Step| {
        if !matches!(step, project::Step::Checking { .. }) {
            eprintln!("{step:?}");
        }
    };
    let opened = if remote {
        project::open_git(
            &project::git::GitRef::parse(&args.path.to_string_lossy()),
            &opts,
            &log,
        )?
    } else {
        let name = args
            .path
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        project::open(
            &args.path,
            name.as_deref().unwrap_or("project"),
            &opts,
            &log,
        )?
    };
    eprintln!("flags: {}", opened.flags.join(" "));
    for u in &opened.units {
        match &u.skipped {
            None => eprintln!("  ok    {}", u.path),
            Some(why) => eprintln!("  skip  {}: {why}", u.path),
        }
    }
    let json = serde_json::to_string(&opened.snapshot).map_err(|error| error.to_string())?;
    writeln!(std::io::stdout().lock(), "{json}").map_err(|error| error.to_string())
}

/// Emit a frozen `argo-protocol` snapshot, or the freeze violations.
fn protocol(args: &Args, parsed: &main::ParseResult) -> Result<(), String> {
    let graph = parsed
        .ast
        .build_graph(&parsed.source, templates::all())
        .map_err(|error| error.to_string())?;
    let path = args.path.to_string_lossy();
    let (mut graph, source) = main::export::to_protocol(&graph, &parsed.ast, &path);
    graph.freeze().map_err(|violations| {
        let lines: Vec<_> = violations.iter().map(ToString::to_string).collect();
        format!("protocol graph failed freeze checks:\n{}", lines.join("\n"))
    })?;
    let snapshot = argo_protocol::Snapshot::new(graph, vec![source]);
    let json = serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?;
    writeln!(std::io::stdout().lock(), "{json}").map_err(|error| error.to_string())
}
