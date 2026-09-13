//! Folding one source's `(inferred kind, observed value)` pairs into a run.

use surrealdb_types::{Kind, Value};
use surrealql_analyzer_workspace::render_kind;

use crate::relation::{classify, observed_kind, Verdict};
use crate::report::{wide_notes, Finding, Run};

/// Compare every statement of one source and fold the verdicts into `run`.
///
/// `kinds` and `observed` are the same length by construction: the caller has
/// already established that the analyzer and the engine agree on how many
/// statements the source has. Where they disagree the source is skipped, never
/// zipped — a silently truncated comparison would report a shifted pairing as a
/// pile of mismatches.
pub fn source(
    run: &mut Run,
    origin: &'static str,
    relative: &str,
    kinds: &[Option<Kind>],
    observed: &[Result<Value, String>],
    verbose: bool,
) {
    for (index, (kind, value)) in kinds.iter().zip(observed).enumerate() {
        let value = match value {
            Ok(value) => value,
            Err(_) => {
                // The engine refused this statement. Language tests test error
                // paths on purpose, so this is normal and there is nothing to
                // compare against.
                run.tally.engine_error += 1;
                continue;
            }
        };
        let Some(kind) = kind else {
            // The analyzer inferred nothing. A coverage gap, not a wrong
            // answer — but counted, because a mismatch total means little
            // without knowing how much the oracle is not asserting on.
            run.tally.no_kind += 1;
            continue;
        };
        let inferred = render_kind(kind);
        let verdict = classify(value, kind);
        if verbose {
            let (label, detail) = match &verdict {
                Verdict::Exact => ("exact   ", String::new()),
                Verdict::Wider(notes) => ("wider   ", wide_notes(notes)),
                Verdict::Mismatch(report) => ("MISMATCH", report.detail()),
            };
            println!("  {label}  {origin}:{relative}:{index}  inferred={inferred}  {detail}");
        }
        if let Verdict::Mismatch(report) = &verdict {
            run.findings.push(Finding {
                key: format!("{origin}:{relative}:{index}"),
                inferred,
                observed: render_kind(&observed_kind(value)),
                detail: report.detail(),
            });
        }
        run.tally.record(&verdict);
    }
}
