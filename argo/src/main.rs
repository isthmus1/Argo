mod cli;

use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::Args::parse().and_then(cli::run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
