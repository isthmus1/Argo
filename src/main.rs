use std::path::{Path, PathBuf};
use std::process::ExitCode;

use main::{Parser, report, templates};

fn main() -> ExitCode {
    let path = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("tests/fixture.c"), PathBuf::from);

    match run(&path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &Path) -> Result<(), String> {
    let parser = Parser::default();
    let parsed = parser.parse_file(path)?;
    report::header(path, &parser, &parsed);
    report::diagnostics(&parsed);
    if parsed.failed() {
        return Err(format!(
            "{} reported errors, no AST emitted",
            path.display()
        ));
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
