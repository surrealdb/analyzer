//! `check` as a JSON document, for the repository's own tooling.
//!
//! The analyzer ships no binary, but `scripts/oracle.py` still needs to run a
//! check over the real-world corpus and read the findings. An example is the
//! right shape for that: it builds with the crate, it is never installed, and
//! it exercises exactly the public API a host would.
//!
//! Usage: `cargo run -p surrealql-analyzer --example check_json -- [DIR]`.
//! Prints `{ summary, diagnostics }` and exits 1 when any error survives
//! policy, 2 when the analysis could not run.

use std::path::PathBuf;
use std::process::ExitCode;

use surrealql_analyzer::{check, Project};

fn main() -> ExitCode {
    let start = std::env::args()
        .nth(1)
        .map_or_else(|| std::env::current_dir().expect("cwd"), PathBuf::from);
    let project = match Project::discover(&start) {
        Ok(project) => project,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    match check(&project) {
        Ok(report) => {
            println!("{}", report.to_json().expect("the report serializes"));
            if report.passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
