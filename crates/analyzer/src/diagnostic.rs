//! The wire shape of a finding, and the resolved findings a report holds.
//!
//! [`Diagnostic`] is the stable `{ code, severity, source, range, message,
//! help, related }` row an embedding host reads and the JSON document
//! carries. [`Findings`] is what a report keeps behind it: the same findings
//! in their original form, with the source texts, so the rustc-style
//! rendering can draw excerpts on request.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use surrealql_analyzer_diagnostics::{render_code, Finding, Severity};

use crate::style::Styles;

/// One finding, resolved to its effective severity and host coordinates.
///
/// Field names and the byte-offset `range` are stable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// The diagnostic code, rendered at its effective severity (`E1002`).
    pub code: String,
    /// `error`, `warning`, or `hint`, after policy.
    pub severity: &'static str,
    /// The file the finding lands in — a host file, never a registry id —
    /// relative to the project root, with no `file://` scheme. A file outside
    /// the root keeps its absolute path.
    pub source: String,
    /// Byte offsets into the file named by `source`.
    pub range: Range,
    /// What is wrong.
    pub message: String,
    /// Suggested fixes.
    pub help: Vec<String>,
    /// Other locations that explain this one.
    pub related: Vec<Related>,
}

/// A secondary location attached to a [`Diagnostic`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Related {
    /// The file this location is in, spelled like [`Diagnostic::source`].
    pub source: String,
    /// Byte offsets into the file named by `source`.
    pub range: Range,
    /// Why this location matters to the finding.
    pub message: String,
}

/// A half-open byte range within a source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Range {
    /// First byte.
    pub start: u32,
    /// One past the last byte.
    pub end: u32,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.source, self.message)
    }
}

impl Diagnostic {
    /// Flattens one resolved finding into the wire shape, with every source
    /// spelled the way the rendered text spells it.
    pub(crate) fn from_finding(finding: &Finding, severity: Severity, root: &Path) -> Self {
        let range = finding.span().range();
        Self {
            code: render_code(finding.code(), severity),
            severity: severity.as_str(),
            source: display_source(root, &finding.span().source().to_string()),
            range: Range {
                start: range.start(),
                end: range.end(),
            },
            message: finding.message().to_string(),
            help: finding
                .help()
                .iter()
                .map(|help| help.message.clone())
                .collect(),
            related: finding
                .related()
                .iter()
                .map(|related| Related {
                    source: display_source(root, &related.span.source().to_string()),
                    range: Range {
                        start: related.span.range().start(),
                        end: related.span.range().end(),
                    },
                    message: related.message.clone(),
                })
                .collect(),
        }
    }
}

/// One source id, spelled for a consumer: the `file://` scheme dropped and the
/// path shown relative to the project root.
///
/// The same rule the renderer uses, for the same reason and then one more. A
/// finding read as text says `src/app.ts`; the JSON said
/// `file:///Users/…/src/app.ts` for a `.surql` file and a bare absolute path
/// for a host file, so a consumer had to know which kind it was holding, and
/// two machines analyzing the same commit disagreed about every `source` in
/// the document. Relative to the root, they agree.
fn display_source(root: &Path, source: &str) -> String {
    let path = source.strip_prefix("file://").unwrap_or(source);
    crate::project::display_relative(root, Path::new(path))
}

/// The findings a report was built from, kept so it can render them.
#[derive(Debug)]
pub(crate) struct Findings {
    resolved: Vec<(Finding, Severity)>,
    texts: BTreeMap<String, String>,
    root: PathBuf,
}

impl Findings {
    pub(crate) fn new(
        resolved: Vec<(Finding, Severity)>,
        texts: BTreeMap<String, String>,
        root: &Path,
    ) -> Self {
        Self {
            resolved,
            texts,
            root: root.to_path_buf(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.resolved.len()
    }

    pub(crate) fn errors(&self) -> usize {
        self.resolved
            .iter()
            .filter(|(_, severity)| *severity == Severity::Error)
            .count()
    }

    pub(crate) fn diagnostics(&self) -> Vec<Diagnostic> {
        self.resolved
            .iter()
            .map(|(finding, severity)| Diagnostic::from_finding(finding, *severity, &self.root))
            .collect()
    }

    /// One rustc-style block per finding, in report order, with paths shown
    /// relative to the project root.
    pub(crate) fn render(&self, styles: Styles) -> Vec<String> {
        self.resolved
            .iter()
            .map(|(finding, severity)| {
                crate::render::render_finding(finding, *severity, &self.texts, &self.root, styles)
            })
            .collect()
    }
}
