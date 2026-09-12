//! Embedded SurrealQL in host files: collecting it into the workspace, and
//! putting its findings back where the author can act on them.
//!
//! A `db.query("SELECT …")` in a `.ts` or `.svelte` file is a query the
//! client actually runs, so it belongs in the analyzed workspace like any
//! `.surql` file. It enters as a *virtual* source — the registry names it
//! `virtual://src/app.ts#3` — which is exactly the coordinate space a user
//! cannot act on: nobody can open that and find line 12.
//! [`remap_finding_to_host`] converts a finding's spans back through the
//! extractor's segment map so it renders as `src/app.ts:12:5`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use surrealql_analyzer_diagnostics::Finding;
use surrealql_analyzer_embed::EmbeddedQuery;
use surrealql_analyzer_syntax::source::SourceId;
use surrealql_analyzer_syntax::span::{ByteRange, SourceSpan};
use surrealql_analyzer_workspace::Workspace;

/// One embedded query, with the host file it was extracted from.
#[derive(Clone, Debug)]
pub(crate) struct HostQuery {
    /// The virtual source id the query was registered under.
    pub source_id: SourceId,
    /// The extracted query, carrying the segment map back to host coordinates.
    pub query: EmbeddedQuery,
    /// The host file's path, as a source id string.
    pub host_id: String,
}

/// Every embedded query found under a project root, in discovery order —
/// the order the registry lists them in — alongside the text of each host
/// file that carried one.
#[derive(Default, Debug)]
pub(crate) struct HostQueries {
    /// The queries, in discovery order.
    pub queries: Vec<HostQuery>,
    /// Each contributing host file's text, so findings render against real
    /// source.
    pub texts: BTreeMap<String, String>,
}

impl HostQueries {
    /// The query registered under `source_id`, if any.
    pub fn get(&self, source_id: &SourceId) -> Option<&HostQuery> {
        self.queries
            .iter()
            .find(|entry| entry.source_id == *source_id)
    }
}

/// Adds every embedded query found in `host_paths` to `workspace` as a virtual
/// source, recording each host file's text so findings can be rendered against
/// real source.
///
/// Embedded queries in host files (`db.query("…")` in `.ts`/`.svelte`/…)
/// are part of the workspace: they are the queries the client actually runs.
/// `generate` has always analyzed them, so a `check` that ignored them would
/// pass a workspace whose `generate` then fails with errors — CI green, build
/// broken.
///
/// An unreadable host file is skipped rather than fatal: it is not a source
/// the user asked to analyze, and failing the whole run over one unreadable
/// `.ts` would make `check` hostage to anything in the tree.
pub(crate) fn collect(workspace: &mut Workspace, host_paths: &[PathBuf]) -> HostQueries {
    let mut collected = HostQueries::default();
    for path in host_paths {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let host_id = path.display().to_string();
        let extracted = surrealql_analyzer_embed::extract(&host_id, &text);
        if extracted.is_empty() {
            continue;
        }
        for query in extracted {
            let source_id = workspace.add_virtual_source(host_id.clone(), query.text.clone());
            // A `defineLive` string is an ordinary SELECT to the parser and a
            // `LIVE SELECT` to the client. Only the extractor saw which sink it
            // reached, so this is where that gets recorded.
            if query.live {
                workspace.mark_live_query(&source_id);
            }
            collected.queries.push(HostQuery {
                source_id,
                query,
                host_id: host_id.clone(),
            });
        }
        collected.texts.insert(host_id, text);
    }
    collected
}

/// Rebuilds a finding whose span is in embedded-query coordinates so it points
/// at the host file: spans belonging to the embedded query (`embed_source`) are
/// mapped through its segment map and re-sourced to `host_id` (so rendering
/// resolves `host_file:line`); spans in other sources (e.g. a schema file the
/// query references) are left as-is.
pub(crate) fn remap_finding_to_host(
    finding: &Finding,
    query: &EmbeddedQuery,
    embed_source: &SourceId,
    host_id: &str,
) -> Finding {
    let host_sid = SourceId::new(host_id);
    finding.map_spans(|span| {
        if span.source() != embed_source {
            return span.clone();
        }
        let range = span.range();
        let mapped = query.host_span(range.start() as usize..range.end() as usize);
        SourceSpan::new(
            host_sid.clone(),
            ByteRange::new(mapped.start as u32, mapped.end as u32)
                .expect("an embedded query's host span is ordered"),
        )
    })
}
