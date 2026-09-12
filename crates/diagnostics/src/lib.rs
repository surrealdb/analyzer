//! What the analyzer reports, and how a consumer grades it.
//!
//! # A finding
//!
//! A [`Finding`] is one contract violation: a [`FindingCode`], a
//! [`SourceSpan`](surrealql_analyzer_syntax::span::SourceSpan), a message, and
//! optional [`Help`] and [`RelatedInfo`]. It carries its **intrinsic**
//! [`Severity`] — what the violation *is* — and nothing about whether it
//! should fail anyone's build.
//!
//! # Grading it
//!
//! That decision belongs to the consumer, at the consumer's edge. Build a
//! [`PolicyConfig`] (the CLI and LSP both build theirs from
//! `surrealql-analyzer.toml`) and call
//! [`resolve_severity`](PolicyConfig::resolve_severity) on each finding: it
//! applies the per-code and per-family [`LintLevel`]s and the
//! warnings-as-errors promotion, and returns `None` for a finding the policy
//! silences. [`render_code`] then spells the *resolved* code for display
//! (`E1004`, `W4003`, `I7002`).
//!
//! In-source `-- surrealql-analyzer: allow(…)` comments are parsed by
//! [`parse_suppression_directive`]; the analyzer applies them, so most
//! consumers never call it.
//!
//! # The catalog
//!
//! [`catalog`] is the single source of truth for the code list — every code,
//! its intrinsic severity, its default lint level, and the contract it
//! enforces. [`catalog::all`] enumerates it, [`catalog::entry`] looks one up,
//! and [`catalog::finding`] is how the analyzer raises one with the catalog's
//! own severity. The published catalogue page is generated from that table.

pub mod catalog;
mod code;
mod finding;
mod policy;
mod suppression;

pub use code::{FindingCategory, FindingCode};
pub use finding::{Finding, FindingTag, Help, RelatedInfo, Severity};
pub use policy::{LintLevel, PolicyConfig};
pub use suppression::{
    parse_suppression_directive, Suppression, SuppressionParseError, SuppressionTarget,
};

/// Renders a code for display: the severity's letter plus the number
/// (`E1004`, `W4003`, `I7002`) — syntax keeps its `S` prefix. Severity is
/// the *resolved* one, so policy promotion shows as `E`.
pub fn render_code(code: FindingCode, severity: Severity) -> String {
    if code.category() == FindingCategory::Syntax {
        return format!("S{:04}", code.number());
    }
    let letter = match severity {
        Severity::Error => 'E',
        Severity::Warning => 'W',
        Severity::Hint => 'I',
    };
    format!("{letter}{:04}", code.number())
}
