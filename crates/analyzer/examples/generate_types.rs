//! `generate` as a runnable command, for the repository's own examples.
//!
//! The analyzer ships no binary — SurrealKit owns the command line — but
//! `examples/basic` and `examples/sveltekit` commit their generated types, and
//! CI has to be able to reproduce them without installing SurrealKit. An
//! example is the right shape for that: it builds with the crate, it is never
//! installed, and it exercises exactly the public API a host would.
//!
//! Usage:
//! `cargo run -p surrealql-analyzer --example generate_types -- <DIR> <OUT.d.ts>`
//!
//! Exits 1 when an embedded query has an error finding (nothing is written),
//! and 2 when the run could not happen at all.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use surrealql_analyzer::{generate, GenerateError, Project, Styles};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .map_or_else(|| std::env::current_dir().expect("cwd"), PathBuf::from);
    let out = args.next().map(PathBuf::from);

    let project = match Project::discover(&root) {
        Ok(project) => project,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };

    match generate(&project, out.as_deref()) {
        Ok(report) => {
            for warning in report.render_warnings(Styles::plain()) {
                eprintln!("{warning}");
            }
            if let Some(warning) = report.render_missing_client(Styles::plain()) {
                eprintln!("{warning}");
            }
            println!(
                "generated {} ({} quer{})",
                display_relative(project.root(), &report.path),
                report.queries,
                if report.queries == 1 { "y" } else { "ies" }
            );
            ExitCode::SUCCESS
        }
        Err(GenerateError::Blocked(blocked)) => {
            for block in blocked.render(Styles::plain()) {
                eprintln!("{block}");
            }
            eprintln!("{blocked}");
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
