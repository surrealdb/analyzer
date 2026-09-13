//! The type oracle's command line.
//!
//!     cargo run --manifest-path tools/type-oracle/Cargo.toml -- <subcommand>
//!
//! or, from the repository root, `scripts/type-oracle.sh <subcommand>`.

use std::path::PathBuf;
use std::process::ExitCode;

use type_oracle::baseline::{self, Baseline};
use type_oracle::report::{summary, Finding, Run};
use type_oracle::{baseline_path, corpus, langtests, repo_root};

const USAGE: &str = "\
type-oracle — does the value SurrealDB returns inhabit the kind the analyzer inferred?

    check             both sources, gated against tools/type-oracle/baseline.txt (default)
    update            rewrite the baseline, preserving the verdicts already recorded
    language-tests    SurrealDB's own language-test corpus, report only
    corpus            the analyzer's vendored corpus, report only

    --repo <path>     the SurrealDB checkout holding language-tests
                      (default: $SURREALDB_REPO, then ../surrealdb)
    --filter <text>   only files whose path contains <text>
    --verbose         print every statement's verdict, not just the mismatches
";

struct Args {
    command: String,
    repo: Option<PathBuf>,
    filter: Option<String>,
    verbose: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        command: "check".to_string(),
        repo: None,
        filter: None,
        verbose: false,
    };
    let mut raw = std::env::args().skip(1);
    let mut command_seen = false;
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "--repo" => args.repo = Some(PathBuf::from(next(&mut raw, "--repo")?)),
            "--filter" => args.filter = Some(next(&mut raw, "--filter")?),
            "--verbose" | "-v" => args.verbose = true,
            "--help" | "-h" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other if other.starts_with('-') => return Err(format!("unknown flag {other}")),
            other if !command_seen => {
                command_seen = true;
                args.command = other.to_string();
            }
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    Ok(args)
}

fn next(raw: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    raw.next().ok_or_else(|| format!("{flag} needs a value"))
}

/// Tokio worker stack size, mirroring SurrealDB's own language-test runner
/// (`language-tests/src/main.rs`) and honouring the same override.
///
/// The default 2 MiB thread stack is not enough for the engine's parser,
/// planner and executor recursion over this corpus: a debug build overflows it
/// and aborts the whole process, which is how CI found this. Upstream settled
/// on 20 MiB for debug builds; there is no reason to disagree with the people
/// who run this corpus every day.
fn worker_stack_size() -> usize {
    std::env::var("SURREAL_RUNTIME_STACK_SIZE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(if cfg!(debug_assertions) {
            20 * 1024 * 1024
        } else {
            10 * 1024 * 1024
        })
}

fn main() -> ExitCode {
    // `block_on` polls the future on the calling thread, so the thread that
    // drives the runtime needs the same stack its workers get — sizing only
    // the workers would move the overflow rather than fix it.
    let stack = worker_stack_size();
    match std::thread::Builder::new()
        .name("type-oracle".to_string())
        .stack_size(stack)
        .spawn(move || drive(stack))
    {
        Ok(handle) => handle.join().unwrap_or(ExitCode::from(2)),
        Err(error) => {
            eprintln!("type-oracle: could not start: {error}");
            ExitCode::from(2)
        }
    }
}

fn drive(stack: usize) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(stack)
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("type-oracle: could not start a runtime: {error}");
            return ExitCode::from(2);
        }
    };
    runtime.block_on(async_main())
}

async fn async_main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("type-oracle: {error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&args).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("type-oracle: {error}");
            ExitCode::from(2)
        }
    }
}

async fn run(args: &Args) -> Result<ExitCode, String> {
    let root = repo_root();
    let filter = args.filter.as_deref();
    match args.command.as_str() {
        "language-tests" => {
            let tests = langtests::locate(args.repo.as_deref(), &root)?;
            let run = langtests::run(&tests, filter, args.verbose).await?;
            report_only(&[run]);
            Ok(ExitCode::SUCCESS)
        }
        "corpus" => {
            let run = corpus::run(&root, filter, args.verbose).await?;
            report_only(&[run]);
            Ok(ExitCode::SUCCESS)
        }
        command @ ("check" | "update") => {
            let tests = langtests::locate(args.repo.as_deref(), &root)?;
            let runs = vec![
                langtests::run(&tests, filter, args.verbose).await?,
                corpus::run(&root, filter, args.verbose).await?,
            ];
            print!("{}", summary(&runs));
            let findings: Vec<Finding> = runs
                .iter()
                .flat_map(|run| run.findings.iter().cloned())
                .collect();
            if command == "update" {
                update(&findings)
            } else {
                check(&findings, filter.is_some())
            }
        }
        other => Err(format!("unknown subcommand {other}\n\n{USAGE}")),
    }
}

fn report_only(runs: &[Run]) {
    print!("{}", summary(runs));
    for run in runs {
        for finding in &run.findings {
            println!(
                "\n  MISMATCH  {}\n      inferred={}\n      observed={}\n      {}",
                finding.key, finding.inferred, finding.observed, finding.detail
            );
        }
    }
}

fn update(findings: &[Finding]) -> Result<ExitCode, String> {
    let path = baseline_path();
    let recorded = Baseline::read(&path);
    std::fs::write(&path, recorded.render(findings))
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    let untriaged = findings
        .iter()
        .filter(|finding| recorded.verdict(&finding.key).is_none())
        .count();
    println!(
        "\n  wrote {} — {} mismatch(es)",
        path.display(),
        findings.len()
    );
    if untriaged > 0 {
        println!("  {untriaged} need a verdict — search for 'UNTRIAGED'");
    }
    Ok(ExitCode::SUCCESS)
}

/// The gate. New or untriaged mismatches fail; a mismatch that disappeared is
/// reported, because a fix is supposed to move the number and nobody should
/// have to notice that by reading a total.
fn check(findings: &[Finding], filtered: bool) -> Result<ExitCode, String> {
    let path = baseline_path();
    let recorded = Baseline::read(&path);
    let seen: std::collections::BTreeSet<&str> = findings
        .iter()
        .map(|finding| finding.key.as_str())
        .collect();

    let mut failures = 0;
    for finding in findings {
        match recorded.verdict(&finding.key).map(baseline::classify) {
            None => {
                failures += 1;
                println!(
                    "\n  NEW        {}\n      inferred={}\n      observed={}\n      {}",
                    finding.key, finding.inferred, finding.observed, finding.detail
                );
            }
            Some(baseline::Triage::Untriaged) => {
                failures += 1;
                println!("\n  UNTRIAGED  {}\n      {}", finding.key, finding.detail);
            }
            Some(_) => {}
        }
    }
    // A filtered run sees only part of the corpus, so absence proves nothing.
    if !filtered {
        for key in recorded.keys() {
            if !seen.contains(key) {
                println!(
                    "\n  RESOLVED   {key}\n      the analyzer no longer gets this wrong — remove the line"
                );
            }
        }
    }

    let (bugs, expected, untriaged) = recorded.counts();
    println!("\n  baseline: {bugs} BUG, {expected} expected, {untriaged} UNTRIAGED");
    if failures > 0 {
        println!(
            "\n  {failures} mismatch(es) need a verdict. Triage, then: scripts/type-oracle.sh update"
        );
        return Ok(ExitCode::FAILURE);
    }
    println!("  all {} mismatch(es) accounted for", findings.len());
    Ok(ExitCode::SUCCESS)
}
