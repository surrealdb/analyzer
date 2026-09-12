//! The SurrealQL analysis engine: schema extraction, type inference over the
//! lowered AST, and the findings the CLI and the language server report.
//!
//! This crate is the library the `surrealql-analyzer` binary and
//! `surrealql-analyzer-lsp` are built on, and it is meant to be linked
//! directly. Everything named in this module is public on purpose; anything
//! not named here is an implementation detail.
//!
//! # The one path everything else is a variation of
//!
//! Build a [`Workspace`], register the sources, analyze it, read the result.
//!
//! ```
//! use surrealql_analyzer_workspace::{analyze_workspace, Workspace, WorkspaceConfig};
//!
//! let mut workspace = Workspace::new(WorkspaceConfig::default());
//! workspace.add_virtual_source(
//!     "schema.surql".into(),
//!     "DEFINE TABLE person SCHEMAFULL;".into(),
//! );
//! let query =
//!     workspace.add_virtual_source("q.surql".into(), "SELECT nope FROM person;".into());
//!
//! let analysis = analyze_workspace(&workspace);
//! assert!(!analysis.diagnostics.is_empty());
//! assert!(analysis.sources.contains_key(&query));
//! ```
//!
//! **Registration order is analysis order.** A source is analyzed against the
//! definitions every source registered before it contributed, so schema files
//! go in first. [`Workspace::add_file_source`] takes a path,
//! [`Workspace::add_virtual_source`] takes an id and text (what the CLI uses
//! for a query it extracted from a `.ts` file); both return the
//! [`SourceId`](surrealql_analyzer_syntax::source::SourceId) the result is
//! keyed by.
//!
//! # What comes back
//!
//! [`analyze_workspace`] returns a [`WorkspaceAnalysis`]: every finding in one
//! list, the [`SchemaIndex`] the sources defined, and one [`AnalysisOutput`]
//! per source. [`AnalysisOutput`] is the shape most consumers want —
//!
//! | field | what it carries |
//! | --- | --- |
//! | [`diagnostics`](AnalysisOutput::diagnostics) | every [`Finding`](surrealql_analyzer_diagnostics::Finding) for the source, syntax and semantic |
//! | [`statements`](AnalysisOutput::statements) | one [`StatementAnalysis`] per top-level statement, in source order |
//! | [`response_kind`](AnalysisOutput::response_kind) | the source's response type, when exactly one statement responds |
//! | [`inferred_params`](AnalysisOutput::inferred_params) | the [`ParamInference`] for each `$param` the source reads |
//! | [`let_bindings`](AnalysisOutput::let_bindings) | every `LET` binding and its inferred type, at every depth |
//! | [`narrowings`](AnalysisOutput::narrowings) | the regions over which a guard flow-narrowed a binding |
//!
//! Findings carry an *intrinsic* severity. Grading them — warnings-as-errors,
//! per-code lint levels, suppression — is the consumer's job and happens at
//! the consumer's edge, through the [`PolicyConfig`] that
//! [`WorkspaceConfig::policy`] builds. That is why two consumers can disagree
//! about whether the same finding fails a build without disagreeing about the
//! finding.
//!
//! A [`Kind`](surrealdb_types::Kind) is rendered for humans with [`render`]
//! (audience-aware, via [`KindContext`]) or [`render_kind`] (the plain form).
//!
//! # Answering editor questions
//!
//! The language server calls these; nothing about them is LSP-specific.
//! [`hover_at`] and [`definition_at`] answer at a byte offset, [`complete_at`]
//! proposes candidates, [`let_binding_hints`] and [`function_return_hints`]
//! produce inlay hints. Each comes in three forms — over source text, over an
//! already-parsed [`ParsedSource`](surrealql_analyzer_syntax::parse::ParsedSource)
//! (`_parsed`), and over already-lowered statements (`_lowered`) — so a caller
//! that already holds the parse does not pay for another one. The suffixless
//! form is the one to reach for first.
//!
//! # Re-analyzing after an edit
//!
//! A keystroke does not need a whole-workspace analysis. [`analyze_one_source`]
//! re-analyzes a single source against a [`GlobalCatalog`] built once by
//! [`build_global_catalog`]; [`changed_symbols`], [`source_reference_set`],
//! [`source_requires_full_reanalysis`] and
//! [`sources_with_changed_cycle_findings`] decide which *other* sources the
//! edit invalidated, and [`reanalyze_sources`] re-runs exactly those. The LSP's
//! document loop is the worked example.

pub mod analysis;
pub mod analyzer;
pub mod completion;
pub mod config;
pub(crate) mod context_params;
pub mod expression;
pub mod kinds;
pub(crate) mod lattice;
pub mod query;
pub mod render;
pub mod schema;
pub mod source_registry;
pub(crate) mod statement_env;
mod suggest;
mod suppress;

pub use analysis::{
    analyze_one_source, analyze_query, analyze_source, analyze_workspace, build_global_catalog,
    build_workspace_schema, changed_symbols, reanalyze_sources, source_reference_set,
    source_requires_full_reanalysis, sources_with_changed_cycle_findings, AnalysisOutput,
    GlobalCatalog, LetBindingAnalysis, NarrowingAnalysis, ParamInference, SelectModifierAnalysis,
    SourceReferenceSet, StatementAnalysis, SymbolKey, ValueDomain, Workspace, WorkspaceAnalysis,
};
pub use completion::{
    complete_at, completion_context_at, CandidateKind, CompletionCandidate, CompletionContext,
    ContextKind,
};
pub use config::{
    AnalysisConfig, ConfigError, DiagnosticConfig, LintConfig, SourceConfig, TargetVersion,
    Version, WorkspaceConfig,
};
pub use expression::{ExpressionFact, ExpressionValueClass, PartialReason};
pub use query::{
    definition_at, definition_at_lowered, definition_at_parsed, function_return_hints,
    function_return_hints_lowered, function_return_hints_parsed, hover_at, hover_at_lowered,
    hover_at_parsed, let_binding_hints, DefinitionTarget, HoverInfo, TypeHint,
};
pub use render::{render, render_kind, KindContext, Rendered};
pub use schema::{
    AnalyzerDef, FieldDef, FieldPath, FunctionDef, ParamDef, RelationDef, SchemaIndex, TableDef,
};

// Named in the docs above but owned by the diagnostics crate; re-exported so a
// consumer can reach the policy type without a second dependency for one name.
pub use surrealql_analyzer_diagnostics::PolicyConfig;
