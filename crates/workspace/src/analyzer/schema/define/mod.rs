//! `DEFINE` statement analysis dispatch.
//!
//! Fans out over the lowered [`ast::DefineStmt`] to one analyzer per
//! Tier 1 DEFINE kind. `Other` covers the unmodeled long tail
//! (ACCESS/API/BUCKET/CONFIG/...) explicitly.
//!
//! The one contract every modeled kind shares lives here: a definition does
//! not redefine (1022). `OVERWRITE` states the intent to replace and
//! `IF NOT EXISTS` the intent to keep the first, so neither is a duplicate;
//! a plain redefinition is neither, and SurrealDB 3.2 fails it outright.

use surrealdb_types::Kind;
use surrealql_analyzer_syntax::ast;
use surrealql_analyzer_syntax::span::{ByteRange, SourceSpan};

use crate::analyzer::context::AnalysisContext;

pub mod analyzer;
pub mod event;
pub mod field;
pub mod function;
pub mod index;
pub mod param;
pub mod permissions;
pub mod table;

pub(crate) fn analyze_define(ctx: &mut AnalysisContext<'_>, stmt: &ast::DefineStmt) -> Kind {
    match stmt {
        ast::DefineStmt::Table(def) => table::analyze_define_table(ctx, def),
        ast::DefineStmt::Field(def) => field::analyze_define_field(ctx, def),
        ast::DefineStmt::Index(def) => index::analyze_define_index(ctx, def),
        ast::DefineStmt::Event(def) => event::analyze_define_event(ctx, def),
        ast::DefineStmt::Param(def) => param::analyze_define_param(ctx, def),
        ast::DefineStmt::Function(def) => function::analyze_define_function(ctx, def),
        ast::DefineStmt::Analyzer(def) => analyzer::analyze_define_analyzer(ctx, def),
        ast::DefineStmt::Other(_) => Kind::Any,
    }
}

/// What a plain redefinition names, in the two spellings a 1022 needs: the
/// analyzer's own (`` `idx` on `person` ``) and the engine's, which is a bare
/// kind plus name ("The index 'idx' already exists").
pub(crate) struct Redefined<'a> {
    /// The engine's noun for the kind: `table`, `field`, `index`, `event`,
    /// `function`, `param`, `analyzer`. Quoted back in the help.
    pub kind: &'a str,
    /// The name as the engine spells it inside that message — the bare
    /// identifier, `$`-prefixed for a param, `fn::`-prefixed for a function.
    pub name: &'a str,
    /// The rendered subject for the message (`` `person` ``, `` `idx` on
    /// `person` ``).
    pub subject: &'a str,
    /// The statement head that would make the replacement deliberate
    /// (`DEFINE INDEX OVERWRITE idx ON person`).
    pub redefine: &'a str,
}

/// Emits the duplicate-definition finding (1022) for a `DEFINE` that names
/// something already in the catalog without `OVERWRITE`/`IF NOT EXISTS`.
///
/// Nothing is replaced and nothing is silent: SurrealDB 3.2 fails a plain
/// redefinition of every modeled kind — verified on 3.2.3, one probe per kind
/// — with "The &lt;kind&gt; '&lt;name&gt;' already exists". `OVERWRITE` and
/// `IF NOT EXISTS` are the only two spellings it accepts, so the help names
/// both rather than only the replacement.
pub(crate) fn emit_duplicate_definition(
    ctx: &mut AnalysisContext<'_>,
    name_span: ByteRange,
    redefined: &Redefined<'_>,
    existing: SourceSpan,
) {
    let Redefined {
        kind,
        name,
        subject,
        redefine,
    } = redefined;
    let span = SourceSpan::new(ctx.source().clone(), name_span);
    ctx.emit(
        surrealql_analyzer_diagnostics::catalog::finding(
            span,
            1022,
            format!(
                "{subject} is already defined; SurrealDB rejects this DEFINE with \"The {kind} '{name}' already exists\""
            ),
        )
        .with_help(format!(
            "write `{redefine}` to replace the earlier definition, or add `IF NOT EXISTS` to keep it"
        ))
        .with_related(existing, format!("{subject} is defined here")),
    );
}
