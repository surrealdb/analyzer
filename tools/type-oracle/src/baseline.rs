//! The verdict baseline: every mismatch the oracle produces, with a verdict.
//!
//! Modelled on `scripts/oracle.py` and `tests/oracle_baseline.txt`, for the
//! same reason. **The mismatch count is not the invariant.** A mismatch is a
//! place the analyzer is wrong about a type; fixing one *should* move the
//! number, and holding the number flat would pressure a change toward
//! suppressing a correct new finding rather than recording it. The invariant
//! that matters is that every mismatch is accounted for: a new one needs a
//! verdict before the gate goes green, and one that disappears is reported so
//! nobody has to notice it by reading a total.
//!
//! Verdicts:
//!
//! - `BUG — <reason>` — the observed value plainly contradicts the inferred
//!   kind. A TODO on us, and the reason this harness exists.
//! - `expected — <reason>` — the engine's behaviour here is genuinely not
//!   knowable statically, so no inference could have got it right.
//! - anything else, including the placeholder — **UNTRIAGED**, which fails.

use std::collections::BTreeMap;

use crate::report::Finding;

/// The placeholder written for a mismatch nobody has looked at yet.
pub const UNTRIAGED: &str = "UNTRIAGED — triage me: BUG / expected?";

const HEADER: &str = "\
# Type-oracle baseline — every statement where the value SurrealDB actually
# returns does NOT inhabit the kind the analyzer inferred, with a verdict.
#
# A mismatch is an analyzer bug unless the engine's behaviour is unknowable
# statically. A generated client would fail to decode it; a hover would lie.
#
# The count is NOT the invariant — fixing a bug is supposed to move it. What is
# gated: every mismatch has a verdict. A NEW mismatch fails the gate until it is
# triaged. A mismatch that DISAPPEARED is reported, not failed — delete the line.
#
# Verdicts:
#   BUG      — the observed value plainly contradicts the inferred kind
#   expected — the engine's behaviour here cannot be known statically
#
# Keys are `source:file:statement-index` and are stable across changes to the
# inferred kind, so a line keeps its verdict while the analyzer stays wrong in a
# new way.
#
# Regenerate with `scripts/type-oracle.sh update` (verdicts are preserved).
";

/// Recorded mismatches, keyed by [`Finding::key`].
#[derive(Debug, Default)]
pub struct Baseline {
    verdicts: BTreeMap<String, String>,
}

impl Baseline {
    /// Parse a baseline file. An unreadable or absent file is an empty
    /// baseline, which makes every mismatch new — and therefore fails loudly
    /// rather than passing vacuously.
    pub fn parse(text: &str) -> Self {
        let mut verdicts = BTreeMap::new();
        let mut key: Option<String> = None;
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if let Some(verdict) = line.trim().strip_prefix("verdict:") {
                if let Some(key) = key.take() {
                    verdicts.insert(key, verdict.trim().to_string());
                }
            } else if !line.starts_with(char::is_whitespace) {
                key = line.split_whitespace().next().map(str::to_string);
            }
        }
        Self { verdicts }
    }

    /// Read a baseline from disk.
    pub fn read(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    /// The verdict recorded for a key, if any.
    pub fn verdict(&self, key: &str) -> Option<&str> {
        self.verdicts.get(key).map(String::as_str)
    }

    /// Every recorded key.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.verdicts.keys().map(String::as_str)
    }

    /// How many lines carry a real verdict of each kind: `(bug, expected, untriaged)`.
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut counts = (0, 0, 0);
        for verdict in self.verdicts.values() {
            match classify(verdict) {
                Triage::Bug => counts.0 += 1,
                Triage::Expected => counts.1 += 1,
                Triage::Untriaged => counts.2 += 1,
            }
        }
        counts
    }

    /// Rewrite the baseline for `findings`, preserving every verdict already
    /// recorded for a key that is still present.
    pub fn render(&self, findings: &[Finding]) -> String {
        let mut out = String::from(HEADER);
        out.push_str(&format!("\n# total: {} mismatch(es)\n\n", findings.len()));
        for finding in findings {
            let verdict = self.verdict(&finding.key).unwrap_or(UNTRIAGED);
            out.push_str(&format!(
                "{}  inferred={}  observed={}\n    {}\n    verdict: {verdict}\n\n",
                finding.key, finding.inferred, finding.observed, finding.detail
            ));
        }
        out
    }
}

/// What a verdict string means to the gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Triage {
    /// A confirmed analyzer bug. Recorded, not gating.
    Bug,
    /// Unknowable statically. Recorded, not gating.
    Expected,
    /// Nobody has looked at it. Fails.
    Untriaged,
}

/// Read a verdict string. Anything that is not an explicit `BUG` or `expected`
/// is untriaged — including the empty string, so a half-written line fails.
pub fn classify(verdict: &str) -> Triage {
    let verdict = verdict.trim();
    if verdict.starts_with("BUG") {
        Triage::Bug
    } else if verdict.starts_with("expected") {
        Triage::Expected
    } else {
        Triage::Untriaged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(key: &str) -> Finding {
        Finding {
            key: key.to_string(),
            inferred: "int".to_string(),
            observed: "array<int>".to_string(),
            detail: "<response>: observed array<int> does not inhabit int".to_string(),
        }
    }

    #[test]
    fn round_trips_through_render_and_parse() {
        let findings = vec![
            finding("langtests:a/b.surql:0"),
            finding("corpus:q/c.surql:2"),
        ];
        let mut seed = Baseline::default();
        seed.verdicts.insert(
            "langtests:a/b.surql:0".to_string(),
            "BUG — a range slice is read as an index".to_string(),
        );
        let rendered = seed.render(&findings);
        let parsed = Baseline::parse(&rendered);
        assert_eq!(
            parsed.verdict("langtests:a/b.surql:0"),
            Some("BUG — a range slice is read as an index")
        );
        assert_eq!(parsed.verdict("corpus:q/c.surql:2"), Some(UNTRIAGED));
        assert_eq!(parsed.counts(), (1, 0, 1));
        // A second regeneration is a fixed point.
        assert_eq!(parsed.render(&findings), rendered);
    }

    #[test]
    fn a_verdict_survives_a_change_to_the_inferred_kind() {
        let before = Baseline::default().render(&[finding("langtests:a/b.surql:0")]);
        let mut triaged = Baseline::parse(&before);
        triaged.verdicts.insert(
            "langtests:a/b.surql:0".to_string(),
            "expected — the engine picks at runtime".to_string(),
        );
        let mut moved = finding("langtests:a/b.surql:0");
        moved.inferred = "string".to_string();
        let after = triaged.render(&[moved]);
        assert!(after.contains("inferred=string"));
        assert_eq!(
            Baseline::parse(&after).verdict("langtests:a/b.surql:0"),
            Some("expected — the engine picks at runtime")
        );
    }

    #[test]
    fn only_bug_and_expected_count_as_triaged() {
        assert_eq!(classify("BUG — x"), Triage::Bug);
        assert_eq!(classify("expected — x"), Triage::Expected);
        assert_eq!(classify(UNTRIAGED), Triage::Untriaged);
        assert_eq!(classify(""), Triage::Untriaged);
        assert_eq!(classify("probably fine"), Triage::Untriaged);
    }

    #[test]
    fn an_absent_baseline_makes_everything_new() {
        let empty = Baseline::parse("");
        assert_eq!(empty.keys().count(), 0);
        assert_eq!(empty.verdict("langtests:a/b.surql:0"), None);
    }
}
