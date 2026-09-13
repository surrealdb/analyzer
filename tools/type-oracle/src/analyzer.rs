//! The analyzer side of the oracle.
//!
//! Nothing here is test-only plumbing: `analyze_workspace` is the same public
//! entry point SurrealKit and the LSP call, and `StatementAnalysis::response_kind`
//! is the same field `crates/codegen` turns into a generated client's return
//! type. Validating it validates the generated types too.

use std::collections::BTreeMap;

use surrealdb_types::Kind;
use surrealql_analyzer_diagnostics::Severity;
use surrealql_analyzer_workspace::{analyze_workspace, Workspace};

/// What analysis said about one source.
#[derive(Clone, Debug, Default)]
pub struct Analyzed {
    /// One entry per top-level statement, in source order: the response kind,
    /// or `None` where the analyzer inferred nothing at all. `None` is a
    /// coverage gap rather than a wrong answer, and counting it is the only
    /// way to see how much of the corpus the oracle is not yet asserting on.
    pub kinds: Vec<Option<Kind>>,
    /// Error-severity findings, as `CODE message`. A file the engine runs
    /// cleanly but the analyzer rejects is a false positive — a negative-gate
    /// candidate, and a different bug class from a wrong type.
    pub errors: Vec<String>,
}

/// Analyze a set of sources as one workspace, keyed by the names given.
///
/// Registration order is the caller's: schema first, then the source under
/// test, exactly as a real workspace would be laid out.
pub fn analyze(sources: &[(String, String)]) -> BTreeMap<String, Analyzed> {
    let mut workspace = Workspace::default();
    let ids: Vec<_> = sources
        .iter()
        .map(|(name, text)| {
            (
                name.clone(),
                workspace.add_virtual_source(name.clone(), text.clone()),
            )
        })
        .collect();
    let analysis = analyze_workspace(&workspace);
    ids.into_iter()
        .map(|(name, id)| {
            let analyzed = analysis
                .sources
                .get(&id)
                .map(|output| Analyzed {
                    kinds: output
                        .statements
                        .iter()
                        .map(|statement| statement.response_kind.clone())
                        .collect(),
                    errors: output
                        .diagnostics
                        .iter()
                        .filter(|finding| finding.severity() == Severity::Error)
                        .map(|finding| format!("{} {}", finding.code(), finding.message()))
                        .collect(),
                })
                .unwrap_or_default();
            (name, analyzed)
        })
        .collect()
}
