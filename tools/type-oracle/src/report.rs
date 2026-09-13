//! What a run produced: the per-verdict tally, the precision breakdown, and
//! the mismatch lines the baseline is made of.

use std::collections::BTreeMap;

use crate::relation::{Verdict, Wide};

/// One mismatch: a statement where the value SurrealDB returned does not
/// inhabit the kind the analyzer inferred.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// `source:file:stmt-index` — the baseline key. Stable across runs and
    /// across changes to the inferred kind, so a line keeps its verdict when
    /// the analyzer's answer changes but stays wrong.
    pub key: String,
    /// The kind the analyzer inferred, rendered.
    pub inferred: String,
    /// The kind of the value the engine returned, rendered.
    pub observed: String,
    /// Every reason the value failed to inhabit the kind.
    pub detail: String,
}

/// Everything a run counted. Informational except for [`Tally::mismatch`],
/// which is what the baseline accounts for line by line.
#[derive(Clone, Debug, Default)]
pub struct Tally {
    /// Files considered.
    pub files: usize,
    /// The inferred kind is exactly the kind of the observed value.
    pub exact: usize,
    /// The value inhabits the kind, but the kind admits more.
    pub wider: usize,
    /// The value does not inhabit the kind.
    pub mismatch: usize,
    /// The analyzer inferred no response kind at all — a coverage gap, not a
    /// wrong answer, and the reason a bare mismatch count understates the work.
    pub no_kind: usize,
    /// Files the analyzer reported error-severity findings on that the engine
    /// ran cleanly. Negative-gate candidates: the analyzer rejects SurrealQL
    /// the engine accepts.
    pub analyzer_error: usize,
    /// Statements the engine itself refused. Expected in a corpus that tests
    /// error paths on purpose; nothing to compare.
    pub engine_error: usize,
    /// Files skipped, by reason.
    pub skipped: BTreeMap<String, usize>,
    /// Where the kind was wider than it needed to be, by class.
    pub precision: BTreeMap<&'static str, usize>,
}

impl Tally {
    /// Record one `(kind, value)` verdict.
    pub fn record(&mut self, verdict: &Verdict) {
        match verdict {
            Verdict::Exact => self.exact += 1,
            Verdict::Wider(notes) => {
                self.wider += 1;
                for (_, wide) in notes {
                    *self.precision.entry(wide.label()).or_default() += 1;
                }
            }
            Verdict::Mismatch(_) => self.mismatch += 1,
        }
    }

    /// Note a skipped file.
    pub fn skip(&mut self, reason: impl Into<String>) {
        *self.skipped.entry(reason.into()).or_default() += 1;
    }

    /// Statements that produced a verdict.
    pub fn compared(&self) -> usize {
        self.exact + self.wider + self.mismatch
    }
}

/// One source's results.
#[derive(Clone, Debug, Default)]
pub struct Run {
    /// Which source produced this — `langtests` or `corpus`.
    pub source: &'static str,
    /// The counts.
    pub tally: Tally,
    /// Every mismatch, in baseline-key order.
    pub findings: Vec<Finding>,
}

/// The informational summary table. Non-gating on purpose: the invariant is
/// that every mismatch is accounted for, never that a count stays flat.
pub fn summary(runs: &[Run]) -> String {
    let mut out = String::new();
    out.push_str(
        "  source      files  compared    exact    wider  MISMATCH  no-kind  engine-err  analyzer-err\n",
    );
    for run in runs {
        let tally = &run.tally;
        out.push_str(&format!(
            "  {:<10}{:>7}{:>10}{:>9}{:>9}{:>10}{:>9}{:>12}{:>14}\n",
            run.source,
            tally.files,
            tally.compared(),
            tally.exact,
            tally.wider,
            tally.mismatch,
            tally.no_kind,
            tally.engine_error,
            tally.analyzer_error,
        ));
    }
    for run in runs {
        if run.tally.precision.is_empty() && run.tally.skipped.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "\n  {} — precision of the `wider` verdicts (positions, not statements):\n",
            run.source
        ));
        for (class, count) in &run.tally.precision {
            out.push_str(&format!("      {class:<10}{count:>8}\n"));
        }
        if !run.tally.skipped.is_empty() {
            out.push_str(&format!("  {} — files skipped:\n", run.source));
            for (reason, count) in &run.tally.skipped {
                out.push_str(&format!("      {count:>6}  {reason}\n"));
            }
        }
    }
    out
}

/// Render a `wider` verdict's account for `--verbose`.
pub fn wide_notes(notes: &[(String, Wide)]) -> String {
    notes
        .iter()
        .map(|(path, wide)| {
            let at = if path.is_empty() { "<response>" } else { path };
            match wide {
                Wide::Union(n) => format!("{at}: union of {n}"),
                other => format!("{at}: {}", other.label()),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
