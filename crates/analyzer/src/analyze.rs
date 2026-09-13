//! The one pipeline both verbs run: load the project's sources into a
//! workspace, analyze it, and resolve every finding through policy onto the
//! coordinates a user can act on.
//!
//! `check` and `generate` used to each build this by hand, and the two copies
//! had already drifted — one registered `.surql` files as `file://` sources
//! and kept their text, the other as `virtual://` and dropped it, so the same
//! finding rendered with a source excerpt under one verb and as bare offsets
//! under the other. One function, one answer.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::PathBuf;

use surrealql_analyzer_diagnostics::{Finding, FindingCode, Severity};
use surrealql_analyzer_syntax::span::{ByteRange, SourceSpan};
use surrealql_analyzer_workspace::{analyze_workspace, Workspace, WorkspaceAnalysis};

use crate::host::{self, HostQueries};
use crate::project::Project;

/// A source file could not be read, or an output could not be written.
///
/// Errors *found* by analysis are never this — they are a result, carried in
/// the report. This is a run that could not happen.
#[derive(Debug)]
pub struct SourceError {
    /// The path that failed.
    pub path: PathBuf,
    /// The underlying I/O error.
    pub source: std::io::Error,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.source)
    }
}

impl std::error::Error for SourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// A project after analysis, before policy.
pub(crate) struct Analyzed {
    /// The workspace analysis: per-source outputs and every finding raised.
    pub analysis: WorkspaceAnalysis,
    /// The embedded queries, keyed back to the host files they came from.
    pub embedded: HostQueries,
    /// Text of every source a finding can point into — `.surql` files by
    /// their registry id, host files by path — so rendering can excerpt it.
    pub texts: BTreeMap<String, String>,
}

/// One finding that survived policy, at its effective severity, in host
/// coordinates.
pub(crate) struct Resolved {
    /// The finding, re-spanned onto the host file if it came from an
    /// embedded query.
    pub finding: Finding,
    /// The severity after policy — promotion, lint levels.
    pub severity: Severity,
    /// Whether the finding was raised on a query embedded in a host file
    /// rather than on a `.surql` source.
    pub embedded: bool,
}

/// Loads every source `project` discovers and analyzes the workspace.
pub(crate) fn analyze(project: &Project) -> Result<Analyzed, SourceError> {
    let sources = project.sources();
    let mut workspace = Workspace::new(project.config().clone());
    let mut texts = BTreeMap::new();

    let mut undecodable = Vec::new();

    for path in sources.surrealql {
        let bytes = fs::read(&path).map_err(|source| SourceError {
            path: path.clone(),
            source,
        })?;
        // A file that is not UTF-8 is reported and skipped, not fatal. One
        // stray latin-1 or binary file used to abort the whole run with exit
        // 2 and zero diagnostics, so a hundred good files went unanalyzed
        // because of a byte in the hundred-and-first. The analyzer cannot
        // guess an encoding — decoding lossily would move every span after
        // the bad byte and report findings on text the file does not contain
        // — so the file contributes nothing but this one finding.
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => {
                let offset = error.utf8_error().valid_up_to();
                let source_id = workspace.add_file_source(path, String::new());
                texts.insert(source_id.to_string(), String::new());
                undecodable.push(undecodable_finding(&source_id, offset));
                continue;
            }
        };
        let source_id = workspace.add_file_source(path, text.clone());
        texts.insert(source_id.to_string(), text);
    }

    let mut embedded = host::collect(&mut workspace, &sources.host);
    texts.append(&mut embedded.texts);

    let mut analysis = analyze_workspace(&workspace);
    analysis.diagnostics.extend(undecodable);

    Ok(Analyzed {
        analysis,
        embedded,
        texts,
    })
}

/// The one finding a file that is not UTF-8 text raises, at the first byte
/// that is not.
///
/// `S0001` rather than a hint: the file names itself a SurrealQL source and
/// the analyzer could not read a character of it, which is the same class of
/// breakage as a source it could not parse, and a hint would let a file the
/// tool is silently ignoring pass CI.
fn undecodable_finding(
    source_id: &surrealql_analyzer_syntax::source::SourceId,
    offset: usize,
) -> Finding {
    let at = u32::try_from(offset).unwrap_or(u32::MAX);
    let span = SourceSpan::new(
        source_id.clone(),
        ByteRange::new(0, 0).expect("zero-width byte range is valid"),
    );
    Finding::new(
        span,
        FindingCode::syntax(1),
        Severity::Error,
        format!("not UTF-8 text: byte {at} is not valid UTF-8, so this file was skipped"),
    )
    .with_help("save the file as UTF-8, or drop it from the `[sources]` globs")
}

impl Analyzed {
    /// Every finding that survives the project's policy.
    ///
    /// Findings carry their intrinsic class; presentation policy
    /// (warnings-as-errors, per-code/family lint levels, suppression) applies
    /// here, at the consumption edge — built once, shared with the LSP. A
    /// finding raised on an embedded query carries registry coordinates that
    /// mean nothing to the user; it is rewritten onto the host file so it
    /// reads as `app.ts:12:5`.
    pub(crate) fn resolve(&self, project: &Project) -> Vec<Resolved> {
        let policy = project.config().policy();
        self.analysis
            .diagnostics
            .iter()
            .filter_map(|finding| {
                let severity = policy.resolve_severity(finding.code(), finding.severity())?;
                let resolved = match self.embedded.get(finding.span().source()) {
                    Some(entry) => Resolved {
                        finding: host::remap_finding_to_host(
                            finding,
                            &entry.query,
                            &entry.source_id,
                            &entry.host_id,
                        ),
                        severity,
                        embedded: true,
                    },
                    None => Resolved {
                        finding: finding.clone(),
                        severity,
                        embedded: false,
                    },
                };
                Some(resolved)
            })
            .collect()
    }
}
