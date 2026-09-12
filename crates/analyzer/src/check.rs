//! `check`: analyze a project's sources and report every finding that
//! survives policy.
//!
//! The verb returns *data*, not text. A completed analysis is
//! [`Ok`]`(`[`CheckReport`]`)` whether or not it found errors — errors are a
//! result, not a failure — and [`CheckReport::passed`] is what a host turns
//! into an exit code. Only a run that could not happen at all (an unreadable
//! source) is [`Err`]. Rendering is a separate, opt-in step:
//! [`CheckReport::render`].

use serde::Serialize;

use crate::analyze::{analyze, SourceError};
use crate::diagnostic::{Diagnostic, Findings};
use crate::project::Project;
use crate::style::Styles;

/// How many sources were analyzed, and what came back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CheckSummary {
    /// Sources the analyzer consumed: every `.surql` file and every embedded
    /// query.
    pub sources_checked: usize,
    /// Findings that survived policy, at any severity.
    pub diagnostics: usize,
    /// The subset of those graded as errors.
    pub errors: usize,
}

/// The JSON document a host emits: one object, one exit code.
#[derive(Serialize)]
struct CheckJson<'a> {
    summary: &'a CheckSummary,
    diagnostics: &'a [Diagnostic],
}

/// A completed analysis.
#[derive(Debug)]
pub struct CheckReport {
    /// Counts for the run.
    pub summary: CheckSummary,
    /// Every finding that survived policy, in host coordinates.
    pub diagnostics: Vec<Diagnostic>,
    findings: Findings,
}

impl CheckReport {
    /// Whether the run is clean: no finding survived policy as an error.
    pub fn passed(&self) -> bool {
        self.summary.errors == 0
    }

    /// One rustc-style block per finding, in report order.
    ///
    /// `styles` decides colour. The caller owns that decision because only it
    /// knows which stream the text is going to: the same finding rendered
    /// plain and rendered coloured differs only by escape sequences, which is
    /// what makes piping to a file safe.
    pub fn render(&self, styles: Styles) -> Vec<String> {
        self.findings.render(styles)
    }

    /// The stable `{ summary, diagnostics }` document, pretty-printed.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&CheckJson {
            summary: &self.summary,
            diagnostics: &self.diagnostics,
        })
    }
}

/// Analyzes every source `project` discovers — `.surql` files and the
/// SurrealQL embedded in host files — and resolves each finding through the
/// project's policy.
pub fn check(project: &Project) -> Result<CheckReport, SourceError> {
    let analyzed = analyze(project)?;
    let sources_checked = analyzed.analysis.sources.len();
    let resolved = analyzed
        .resolve(project)
        .into_iter()
        .map(|resolved| (resolved.finding, resolved.severity))
        .collect();
    let findings = Findings::new(resolved, analyzed.texts, project.root());

    Ok(CheckReport {
        summary: CheckSummary {
            sources_checked,
            diagnostics: findings.len(),
            errors: findings.errors(),
        },
        diagnostics: findings.diagnostics(),
        findings,
    })
}
