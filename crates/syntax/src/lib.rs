//! SurrealQL syntax: parsing, the typed span-carrying AST, and the lowering
//! pass between them.
//!
//! This crate owns everything that reads source text. Analysis consumes only
//! [`ast`] values, which is what lets the analyzer run without a parser in
//! scope — and what lets the language server re-use one parse across hover,
//! completion and diagnostics.
//!
//! # The pipeline
//!
//! ```
//! use surrealql_analyzer_syntax::lower::lower_statements;
//! use surrealql_analyzer_syntax::parse::parse_source;
//! use surrealql_analyzer_syntax::source::SourceId;
//!
//! let parsed = parse_source(SourceId::new("q.surql"), "SELECT name FROM person;")?;
//! let statements = lower_statements(&parsed);
//! assert_eq!(statements.len(), 1);
//! # Ok::<(), surrealql_analyzer_syntax::parse::ParseError>(())
//! ```
//!
//! 1. [`parse::parse_source`] runs tree-sitter over the text and returns a
//!    [`ParsedSource`](parse::ParsedSource) — the CST, the text, the
//!    [`SourceId`](source::SourceId), and any
//!    [`SyntaxDiagnostic`](parse::SyntaxDiagnostic)s. Hold it: everything
//!    downstream takes it by reference, and re-parsing is the expensive step.
//! 2. [`lower::lower_statements`] turns that CST into typed
//!    [`ast::Statement`]s. [`lower::lower`] produces an [`ast::Script`]
//!    instead, when the source is being treated as one unit.
//! 3. [`highlight::tokens`] produces semantic-highlight tokens over the same
//!    parse, for an editor.
//!
//! # Spans
//!
//! Every AST node is a [`ast::Spanned<T>`](ast::Spanned) carrying a
//! [`ByteRange`](span::ByteRange) into the source text, and a
//! [`SourceSpan`](span::SourceSpan) pairs one with the
//! [`SourceId`](source::SourceId) it belongs to. Spans are byte offsets on
//! UTF-8 boundaries; converting them to an editor's line/column is the
//! consumer's job.
//!
//! # Half-typed input is normal input
//!
//! The language server parses on every keystroke, so most input this crate
//! sees is incomplete. Nothing here panics on it: a construct whose syntax is
//! broken lowers to an explicit [`ast::PartialNode`] rather than being
//! silently dropped, and every span stays in bounds and on a character
//! boundary. `tests/robustness.rs` holds that as a property.

pub mod ast;
pub mod highlight;
pub mod lower;
pub mod parse;
pub mod source;
pub mod span;
