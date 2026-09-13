//! A **type oracle** for the SurrealQL Analyzer: it takes the response kind the
//! analyzer infers for a statement, takes the value SurrealDB actually returns
//! for that same statement, and asks whether the value inhabits the kind.
//!
//! The rest of the quality harness proves that inference is *stable*
//! (`precision_snapshot.rs`), that it does not *degrade* (`any_ratchet.rs`,
//! `narrowing_floor.rs`), and that the grammar accepts what the engine accepts
//! (`conformance.rs`). None of them can say whether an inferred type is
//! **true** — a snapshot records what inference says, and a snapshot of a wrong
//! answer is a green test. Only a running engine can settle that.
//!
//! Two sources of `(schema, query, data)` triples:
//!
//! - [`langtests`] — SurrealDB's own language-test corpus, maintained by the
//!   engine team. Each file is schema and queries in one source, and its
//!   `[[test.results]]` entries line up 1:1 with the analyzer's statements.
//! - [`corpus`] — the analyzer's own vendored corpus, executed instead of only
//!   analyzed.
//!
//! The gate is [`baseline`]: a verdict per mismatch, never a count. See that
//! module for why.

pub mod analyzer;
pub mod baseline;
pub mod compare;
pub mod corpus;
pub mod engine;
pub mod header;
pub mod langtests;
pub mod relation;
pub mod report;

use std::path::{Path, PathBuf};

/// The analyzer repository root, derived from this crate's location
/// (`tools/type-oracle`).
pub fn repo_root() -> PathBuf {
    match Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
    {
        Some(root) => root.to_path_buf(),
        None => PathBuf::from("."),
    }
}

/// Where the committed verdict baseline lives.
pub fn baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("baseline.txt")
}
