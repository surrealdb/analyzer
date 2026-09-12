//! The SurrealQL analyzer tree: one module per statement, expression, or
//! function family, each owning its own analysis logic.
//!
//! Analyzers consume the typed AST from `surrealql_analyzer_syntax::ast` (lowered
//! once per source by `surrealql_analyzer_syntax::lower`) and infer upstream
//! `surrealdb_types::Kind` response types: closed objects are
//! `Kind::Literal(KindLiteral::Object(..))`, and undeterminable positions
//! are `Kind::Any` poison values. Diagnostics are appended through the
//! shared [`context::AnalysisContext`]; per-statement invariants belong to
//! the analyzer that owns their statement.
//!
//! [`pipeline`] is the entry point: it walks each source in statement
//! order, dispatching every lowered statement to its analyzer against the
//! schema built so far.

// The engine's internals. Only [`contract`] is public, and only because the
// contract table it holds is an enumeration of every checked position — a
// thing a consumer can legitimately want to enumerate, and what
// `tests/contract_positions.rs` holds the catalog against. Everything else
// here is an implementation detail of `analyze_workspace`: making these
// `pub` again would re-expose ~500 modules that no consumer has ever named.
pub(crate) mod const_eval;
pub(crate) mod context;
pub mod contract;
pub(crate) mod data;
pub(crate) mod expression;
pub(crate) mod facts;
pub(crate) mod flow;
pub(crate) mod function;
pub(crate) mod omit;
pub(crate) mod pipeline;
pub(crate) mod schema;
pub(crate) mod statement;
pub(crate) mod system;
pub(crate) mod version;

#[cfg(test)]
pub(crate) mod test_support;
