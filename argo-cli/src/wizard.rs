//! Small prompts for what the command line left out, only when a person is at a terminal.

use std::io::IsTerminal;

use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, FuzzySelect, Input};

use crate::runs::Run;

/// Whether prompts can be shown and answered.
pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

fn cancelled(e: dialoguer::Error) -> String {
    format!("prompt cancelled ({e})")
}

pub fn repo() -> Result<String, String> {
    Input::with_theme(&theme())
        .with_prompt("Repository (directory, or git URL[#ref])")
        .validate_with(|s: &String| match s.trim().is_empty() {
            true => Err("a repository is needed"),
            false => Ok(()),
        })
        .interact_text()
        .map(|s| s.trim().to_string())
        .map_err(cancelled)
}

pub fn jobs() -> Result<u32, String> {
    Input::with_theme(&theme())
        .with_prompt("Workers")
        .default(4)
        .validate_with(|n: &u32| if *n >= 1 { Ok(()) } else { Err("at least 1") })
        .interact_text()
        .map_err(cancelled)
}

pub fn confirm(prompt: &str, default: bool) -> Result<bool, String> {
    Confirm::with_theme(&theme())
        .with_prompt(prompt)
        .default(default)
        .interact()
        .map_err(cancelled)
}

/// Pick one run: arrows move, typing filters, Enter launches.
pub fn pick(runs: &[Run]) -> Result<usize, String> {
    let width = runs.iter().map(|r| r.name.len()).max().unwrap_or(0);
    let lines: Vec<String> = runs.iter().map(|r| r.line(width)).collect();
    FuzzySelect::with_theme(&theme())
        .with_prompt("Replay which run? (type to filter, Enter to launch)")
        .items(&lines)
        .default(0)
        .max_length(12)
        .interact()
        .map_err(cancelled)
}
