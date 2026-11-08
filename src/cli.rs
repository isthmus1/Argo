use std::io::Write;
use std::path::PathBuf;

use main::{Parser, report, templates};

pub(crate) struct Args {
    path: PathBuf,
    graph_json: bool,
}

impl Args {
    pub fn parse() -> Result<Self, String> {
        let (mut path, mut graph_json, mut positional) = (None, false, false);
        for arg in std::env::args_os().skip(1) {
            if !positional && arg == "--" {
                positional = true;
            } else if !positional && arg == "--graph-json" {
                graph_json = true;
            } else if !positional && arg.to_str().is_some_and(|arg| arg.starts_with('-')) {
                return Err(format!("unknown option {}", arg.to_string_lossy()));
            } else if path.replace(PathBuf::from(arg)).is_some() {
                return Err("expected at most one C input path".into());
            }
        }
        Ok(Self {
            path: path.unwrap_or_else(|| "tests/fixture.c".into()),
            graph_json,
        })
    }
}

pub(crate) fn run(args: Args) -> Result<(), String> {
    let parser = Parser::default();
    let parsed = parser.parse_file(&args.path)?;
    if !args.graph_json {
        report::header(&args.path, &parser, &parsed);
    }
    report::diagnostics(&parsed);
    if parsed.failed() {
        return Err(format!(
            "{} reported errors, no AST emitted",
            args.path.display()
        ));
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
