//! `DEFINE TABLE` analysis.
//!
//! A table is defined once: redefining it without `OVERWRITE` or `IF NOT
//! EXISTS` is a duplicate definition (1022).

use surrealdb_types::{Kind, Table};
use surrealql_analyzer_syntax::ast;

use crate::analyzer::context::AnalysisContext;

pub(crate) fn analyze_define_table(ctx: &mut AnalysisContext<'_>, stmt: &ast::DefineTable) -> Kind {
    // `IF NOT EXISTS` makes a redefinition a deliberate no-op, exactly as
    // `OVERWRITE` makes it a deliberate replacement.
    if !stmt.overwrite && !stmt.if_not_exists {
        if let Some(existing) = ctx.schema().table(&stmt.name.node) {
            let existing = existing.name_span.clone();
            // The additive pre-pass makes every OTHER source look like it
            // already defines `name`, regardless of registration order — only
            // a genuine predecessor (this same source, earlier, or a source
            // that truly precedes it) is one this statement redefines; the
            // other side of a cross-file pair reports it, once, from there.
            if ctx.source_precedes(existing.source()) {
                super::emit_duplicate_definition(
                    ctx,
                    stmt.name.span,
                    &super::Redefined {
                        kind: "table",
                        name: &stmt.name.node,
                        subject: &format!("`{}`", stmt.name.node),
                        redefine: &format!("DEFINE TABLE OVERWRITE {}", stmt.name.node),
                    },
                    existing,
                );
            }
        }
    }

    // Each `PERMISSIONS FOR <action> WHERE <expr>` predicate is evaluated
    // against a row of this table; `$value` is that record.
    let record = Kind::Record(vec![Table::from(stmt.name.node.as_str())]);
    super::permissions::analyze_permission_predicates(
        ctx,
        &stmt.name.node,
        record,
        &stmt.permissions,
    );

    if let Some(view) = &stmt.view {
        check_view_sources(ctx, view);
    }

    Kind::None
}

/// A view's `FROM` targets are read like any other SELECT's: a name that
/// names no table is 1001, exactly as `DEFINE TABLE stats AS SELECT * FROM
/// nosuchtable;` fails on the engine ("The table 'nosuchtable' does not
/// exist") while today's analyzer says nothing at all — the view body was not
/// analyzed. Only the plain `FROM name` shape resolves here (a bare `Ident`
/// lowers straight to `Expr::Table`); anything else stays silent rather than
/// guess.
fn check_view_sources(ctx: &mut AnalysisContext<'_>, view: &ast::ViewClause) {
    for from in &view.from {
        if let ast::Expr::Table(name) = &from.node {
            crate::analyzer::data::check_table_reference(ctx, &name.node, name.span);
        }
    }
}
