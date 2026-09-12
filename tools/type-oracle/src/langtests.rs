//! The language-test source: SurrealDB's own `.surql` test corpus.
//!
//! This is the best oracle input that exists, for three reasons. The files are
//! maintained by the engine team, so they track the engine rather than our idea
//! of it. Each file is schema *and* queries in one source, which is exactly the
//! shape `analyze_workspace` takes. And each file's `[[test.results]]` entries
//! line up 1:1 with `AnalysisOutput.statements`, so no alignment heuristics are
//! needed — when the counts disagree, that itself is worth knowing and the file
//! is skipped rather than guessed at.
//!
//! Observed values come from **running the file**, not from evaluating the
//! recorded literal. The recorded literal is used for what it is better at:
//! confirming the statement counts agree.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::analyzer::analyze;
use crate::engine::Engine;
use crate::header::{self, Header};
use crate::report::Run;

/// Locate the SurrealDB checkout holding the language tests.
///
/// `--repo` wins, then `SURREALDB_REPO`, then a sibling checkout next to this
/// repository — which is where a SurrealDB contributor already has one.
pub fn locate(explicit: Option<&Path>, repo_root: &Path) -> Result<PathBuf, String> {
    let candidates: Vec<PathBuf> = explicit
        .map(Path::to_path_buf)
        .into_iter()
        .chain(
            std::env::var_os("SURREALDB_REPO")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        )
        .chain(repo_root.parent().map(|parent| parent.join("surrealdb")))
        .collect();
    for candidate in &candidates {
        if let Some(tests) = tests_dir(candidate) {
            return Ok(tests);
        }
    }
    Err(format!(
        "no SurrealDB checkout with language tests found (looked in: {}).\n\
         Point the oracle at one:\n    \
         git clone --depth 1 --branch v3.2.3 https://github.com/surrealdb/surrealdb ../surrealdb\n\
         or set SURREALDB_REPO=/path/to/surrealdb, or pass --repo /path/to/surrealdb.",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// The tests directory inside a checkout. The crate moved between releases, so
/// both layouts are accepted.
fn tests_dir(repo: &Path) -> Option<PathBuf> {
    ["language-tests/tests", "crates/language-tests/tests"]
        .iter()
        .map(|relative| repo.join(relative))
        .find(|path| path.is_dir())
}

/// Run the whole language-test corpus through the oracle.
pub async fn run(tests: &Path, filter: Option<&str>, verbose: bool) -> Result<Run, String> {
    let engine_version = Engine::fresh().await?.version().await?;
    let mut run = Run {
        source: "langtests",
        ..Run::default()
    };
    for path in files(tests) {
        let relative = path
            .strip_prefix(tests)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if filter.is_some_and(|filter| !relative.contains(filter)) {
            continue;
        }
        run.tally.files += 1;
        if let Err(reason) = one(&mut run, tests, &path, &relative, &engine_version, verbose).await
        {
            run.tally.skip(reason);
        }
    }
    run.findings.sort();
    Ok(run)
}

/// Every `.surql` file under the corpus, in a stable order.
fn files(tests: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = walkdir::WalkDir::new(tests)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .map(walkdir::DirEntry::into_path)
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "surql")
        })
        .collect();
    paths.sort();
    paths
}

/// One test file. `Err` is a skip reason, never a failure of the run.
async fn one(
    run: &mut Run,
    tests: &Path,
    path: &Path,
    relative: &str,
    engine_version: &semver::Version,
    verbose: bool,
) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|_| "unreadable".to_string())?;
    let (toml, body) = header::split(&text);
    let header = Header::parse(&toml)?;
    header.supported(engine_version)?;

    // Imports are schema for the analyzer and seed statements for the engine —
    // the same files playing the same two roles they play in a real workspace.
    let mut imports = Vec::new();
    let mut seen = BTreeSet::new();
    collect_imports(tests, path, &header, &mut imports, &mut seen)?;

    let engine = Engine::fresh().await?;
    engine
        .select(header.namespace.as_deref(), header.database.as_deref())
        .await?;
    for (name, text) in &imports {
        engine
            .seed(text)
            .await
            .map_err(|error| format!("an import was rejected by the engine ({name}: {error})"))?;
    }
    let observed = engine
        .run(&body)
        .await
        .map_err(|_| "the engine rejected the file outright".to_string())?;

    let mut sources = imports;
    sources.push((relative.to_string(), body));
    let analyzed = analyze(&sources)
        .remove(relative)
        .ok_or_else(|| "the analyzer produced no output".to_string())?;

    // A file the engine ran cleanly and the analyzer rejects is a false
    // positive — a different bug class from a wrong type, and one this harness
    // gets for free. Its inferred kinds are not trustworthy, so nothing on the
    // file is compared.
    if !analyzed.errors.is_empty() {
        if observed.iter().all(Result::is_ok) {
            run.tally.analyzer_error += 1;
            if verbose {
                println!("  analyzer-error  {relative}  {}", analyzed.errors[0]);
            }
            return Err("the analyzer rejects a file the engine runs".to_string());
        }
        return Err("both the analyzer and the engine reject the file".to_string());
    }

    if let Some(expected) = header.result_count {
        if expected != analyzed.kinds.len() {
            return Err("statement count disagrees with [[test.results]]".to_string());
        }
    }
    if observed.len() != analyzed.kinds.len() {
        return Err("statement count disagrees with the engine".to_string());
    }

    crate::compare::source(
        run,
        "langtests",
        relative,
        &analyzed.kinds,
        &observed,
        verbose,
    );
    Ok(())
}

/// Resolve `[env] imports` depth-first, so an import's own imports land first.
fn collect_imports(
    tests: &Path,
    path: &Path,
    header: &Header,
    out: &mut Vec<(String, String)>,
    seen: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    for import in &header.imports {
        let resolved = if import.starts_with("./") || import.starts_with("../") {
            path.parent()
                .ok_or_else(|| "an import has no parent directory".to_string())?
                .join(import)
        } else {
            tests.join(import)
        };
        let resolved = resolved
            .canonicalize()
            .map_err(|_| format!("an import does not exist ({import})"))?;
        if !seen.insert(resolved.clone()) {
            continue;
        }
        let text = std::fs::read_to_string(&resolved)
            .map_err(|_| format!("an import is unreadable ({import})"))?;
        let (toml, body) = header::split(&text);
        let nested = Header::parse(&toml)?;
        collect_imports(tests, &resolved, &nested, out, seen)?;
        out.push((import.clone(), body));
    }
    Ok(())
}
