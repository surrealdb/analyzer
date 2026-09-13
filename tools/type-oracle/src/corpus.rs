//! The analyzer's own corpus as an oracle source.
//!
//! `crates/workspace/tests/precision_snapshot.rs` records, for this corpus,
//! every type inference produces — and its own header says it records *current*
//! behaviour, not correct behaviour. This closes that loop: the same corpus,
//! the same `analyze_workspace` call, but the claim is checked against what the
//! engine returns when the queries are actually run.
//!
//! The corpus was written to be analyzed, not executed, so it carries no seed
//! data and reads host parameters the engine has never heard of. Both show up
//! honestly in the tally — as `no-kind`, `engine-err`, and empty-array `wider`
//! verdicts — rather than being hidden.

use std::path::{Path, PathBuf};

use crate::analyzer::analyze;
use crate::compare;
use crate::engine::Engine;
use crate::report::Run;

/// Run the analyzer's corpus through the oracle.
pub async fn run(repo_root: &Path, filter: Option<&str>, verbose: bool) -> Result<Run, String> {
    let root = repo_root.join("crates/workspace/tests/corpus");
    let schema = read_dir(&root.join("schema"))?;
    let queries = read_dir(&root.join("queries"))?;
    if schema.is_empty() || queries.is_empty() {
        return Err(format!(
            "the corpus at {} is empty — the oracle would pass vacuously",
            root.display()
        ));
    }

    // One workspace, exactly as `tests/support/mod.rs::analyze_corpus` builds
    // it: schema first, then queries, so the schema index is complete before
    // any query is analyzed.
    let mut sources = schema.clone();
    sources.extend(queries.clone());
    let analyzed = analyze(&sources);

    let mut run = Run {
        source: "corpus",
        ..Run::default()
    };
    let mut refused = std::collections::BTreeSet::new();
    for (name, text) in &queries {
        if filter.is_some_and(|filter| !name.contains(filter)) {
            continue;
        }
        run.tally.files += 1;
        // A fresh database per query file, reseeded from the schema. The corpus
        // contains mutations; without this, one file's writes would silently
        // become another file's inputs.
        let engine = Engine::fresh().await?;
        engine.select(Some("test"), Some("test")).await?;
        for (schema_name, schema_text) in &schema {
            // A schema file the engine will not even parse is a finding, not a
            // reason to abandon the run: the grammar accepted SurrealQL the
            // engine rejects, which is what `engine_refused_syntax.rs` exists
            // to pin. Report it once and carry on with a partial schema.
            if let Err(error) = engine.seed(schema_text).await {
                if refused.insert(schema_name.clone()) {
                    println!("  engine refuses the corpus schema {schema_name}: {error}");
                }
                run.tally.skip(format!(
                    "the engine rejects the corpus schema {schema_name}"
                ));
            }
        }
        let observed = match engine.run(text).await {
            Ok(observed) => observed,
            Err(_) => {
                run.tally.skip("the engine rejected the file outright");
                continue;
            }
        };
        let Some(source) = analyzed.get(name) else {
            run.tally.skip("the analyzer produced no output");
            continue;
        };
        if !source.errors.is_empty() {
            // The corpus is all-valid by construction, so this is a finding in
            // itself rather than a routine skip.
            run.tally.analyzer_error += 1;
            if verbose {
                println!("  analyzer-error  {name}  {}", source.errors[0]);
            }
            continue;
        }
        if observed.len() != source.kinds.len() {
            run.tally.skip("statement count disagrees with the engine");
            continue;
        }
        compare::source(&mut run, "corpus", name, &source.kinds, &observed, verbose);
    }
    run.findings.sort();
    Ok(run)
}

/// Every `.surql` file in a directory as `(name, text)`, in path order.
fn read_dir(directory: &Path) -> Result<Vec<(String, String)>, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "surql")
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let name = format!(
                "{}/{}",
                directory
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            Ok((name, text))
        })
        .collect()
}
