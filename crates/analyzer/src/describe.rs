//! `describe`: the project's types as data.
//!
//! [`generate`](crate::generate) writes TypeScript. This verb stops one step
//! earlier and hands back the [`TypesDocument`] that TypeScript is rendered
//! from — the same tables, functions, globals and per-query result kinds, in
//! a language-neutral, serializable form.
//!
//! It exists because "generate a client" is not one job. Emitting `.d.ts` is;
//! so is emitting Rust structs, Python dataclasses, or a documentation page,
//! and every one of them needs the same facts. A host that wants any of those
//! calls `describe` and renders the document itself, rather than parsing the
//! TypeScript back out or re-deriving the facts from a second analysis that
//! would drift from this one.

use surrealql_analyzer_codegen::{QueryTypes, Source, TypesDocument};

use crate::analyze::{analyze, Analyzed, SourceError};
use crate::project::Project;

/// Analyzes `project` and describes everything it knows about its types.
///
/// Unlike `generate`, findings do not block: a document is a description, and
/// a project with an error in one query still has a schema and still has the
/// other queries. Callers that need the findings run
/// [`check`](crate::check) — the same analysis, graded.
pub fn describe(project: &Project) -> Result<TypesDocument, SourceError> {
    Ok(document(&analyze(project)?))
}

/// The document for an analysis that has already run, so `generate` describes
/// and renders without analyzing twice.
pub(crate) fn document(analyzed: &Analyzed) -> TypesDocument {
    let queries = analyzed
        .embedded
        .queries
        .iter()
        .filter_map(|entry| {
            let output = analyzed.analysis.sources.get(&entry.source_id)?;
            Some(QueryTypes::from_analysis(entry.query.parts(), output))
        })
        .collect();
    // `Static`: every fact here came from the project's own sources. A
    // document built by introspecting a running database is `Live`, and the
    // two can legitimately disagree — an unapplied migration is exactly that
    // disagreement — so a consumer is told which it holds.
    TypesDocument::new(Source::Static, &analyzed.analysis.schema, queries)
}
