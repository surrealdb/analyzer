//! SurrealQL Analyzer — the library a host embeds.
//!
//! The engine crates (`surrealql-analyzer-{syntax,workspace,diagnostics,embed,
//! codegen}`) analyze sources they are handed. This crate is the layer above
//! them: it knows what a *project* is — which files on disk are schema, which
//! are queries, which host files carry embedded SurrealQL — and it runs the
//! three verbs a development loop needs over that project.
//!
//! * [`check`] — analyze every source and return each finding that survives
//!   the project's policy, as data.
//! * [`generate`] — emit the typed TypeScript client and literal-keyed query
//!   registry for the embedded queries, refusing to overwrite a good registry
//!   with a broken one.
//! * [`watch_loop`] — re-run a caller's closure on every change to an input
//!   the analysis consumes, debounced so one save is one run.
//!
//! # Who calls this
//!
//! SurrealKit's `check`, `generate` and `watch` commands. The language server
//! consumes the `workspace` crate directly, because an editor has different
//! needs (per-document incremental analysis, positions rather than byte
//! ranges). This crate has no binary and parses no arguments: how a verb is
//! spelled on a command line, what its exit code is, and how its output is
//! laid out on a terminal are a host's decisions.
//!
//! # Configuration
//!
//! A [`Project`] is a root plus a [`WorkspaceConfig`](workspace::config::WorkspaceConfig).
//! A host that already knows where its schema lives builds the config itself
//! and calls [`Project::new`]; nothing here requires a `surrealql-analyzer.toml`.
//! [`Project::discover`] still reads one for the workspaces that have it.
//!
//! # Output
//!
//! Verbs return structs, not strings. Each report also offers a rustc-style
//! rendering — source excerpt, caret underline, `help:` lines — through
//! [`CheckReport::render`] and friends, with colour as a [`Styles`] parameter
//! the caller sets from whatever it knows about its destination stream.

mod analyze;
pub mod check;
pub mod diagnostic;
pub mod generate;
mod host;
pub mod project;
pub mod render;
pub mod style;
#[cfg(feature = "watch")]
mod watch;

pub use analyze::SourceError;
pub use check::{check, CheckReport, CheckSummary};
pub use diagnostic::{Diagnostic, Range, Related};
pub use generate::{generate, GenerateBlocked, GenerateError, GenerateReport, CLIENT_PACKAGE};
pub use project::{ConfigError, Project, Sources, CONFIG_FILE_NAME, DEFAULT_REGISTRY_NAME};
pub use render::render_finding;
pub use style::Styles;
#[cfg(feature = "watch")]
pub use watch::{watch_loop, ChangeSet, WatchRun};

// The engine crates, re-exported so a host depends on one crate and the
// versions can never drift apart.
pub use surrealql_analyzer_codegen as codegen;
pub use surrealql_analyzer_diagnostics as diagnostics;
pub use surrealql_analyzer_embed as embed;
pub use surrealql_analyzer_syntax as syntax;
pub use surrealql_analyzer_workspace as workspace;
