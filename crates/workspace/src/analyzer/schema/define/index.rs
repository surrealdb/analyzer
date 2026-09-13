//! `DEFINE INDEX` analysis.
//!
//! An index must target a known table (1001) over fields that table declares
//! (1002), is defined once (1022), and two indexes over the same field set do
//! the same work twice (1029), and a full-text index tokenizes with an
//! analyzer that exists (1012). The `1012` index-target check for
//! `REBUILD`/`REMOVE INDEX` lives here too, since it is the same catalog
//! reference from the other side.

use surrealdb_types::Kind;
use surrealql_analyzer_syntax::ast;
use surrealql_analyzer_syntax::span::{ByteRange, SourceSpan};

use crate::analyzer::context::AnalysisContext;

pub(crate) fn analyze_define_index(ctx: &mut AnalysisContext<'_>, stmt: &ast::DefineIndex) -> Kind {
    check_count_index_fields(ctx, stmt);

    let refs = crate::schema::index_field_refs(stmt, ctx.source());
    let source = ctx.source().clone();

    let Some(table) = ctx.schema().table(&stmt.table.node) else {
        let finding = surrealql_analyzer_diagnostics::catalog::finding(
            SourceSpan::new(source, stmt.table.span),
            1001,
            format!(
                "index `{}` is defined on `{}`, which is not a defined table",
                stmt.name.node, stmt.table.node
            ),
        );
        let finding = crate::analyzer::data::with_table_suggestion(finding, ctx, &stmt.table.node);
        ctx.emit(finding);
        return Kind::None;
    };

    // Same gate as every other 1002: only a SCHEMAFULL table has a closed
    // field set to check against. A schemaless table accepts any field, and a
    // view's columns are its projection's aliases, which the schema index does
    // not carry — `DEFINE INDEX itotal ON stats FIELDS total` over
    // `DEFINE TABLE stats AS SELECT count() AS total …` builds on 3.2.3.
    let mut unknown_fields = Vec::new();
    if table.schemafull {
        for (path, text, span) in &refs {
            if !crate::schema::index_field_path_exists_on_table(table, path) {
                unknown_fields.push((text.clone(), span.clone()));
            }
        }
    }

    // Two indexes over the same field set do the same work twice (1029).
    let mut paths: Vec<String> = refs.iter().map(|(path, _, _)| path.join(".")).collect();
    paths.sort();
    let this_kind = crate::schema::index_def_from_ast(stmt, ctx.source()).kind;
    let duplicate = (!paths.is_empty())
        .then(|| {
            table.indexes.values().find(|other| {
                let mut other_paths = other.field_paths();
                other_paths.sort();
                // A UNIQUE index and a plain index over the same fields do
                // different work (one enforces a constraint, the other only
                // speeds lookups), so they are not redundant. Two indexes are
                // duplicates only when their backing kind matches too.
                other.name != stmt.name.node && other.kind == this_kind && other_paths == paths
            })
        })
        .flatten()
        .map(|other| other.name.clone());

    let table_name_span = table.name_span.clone();
    let field_keys: Vec<String> = table.fields.keys().cloned().collect();
    let existing = (!stmt.overwrite && !stmt.if_not_exists)
        .then(|| table.indexes.get(&stmt.name.node))
        .flatten()
        .map(|existing| existing.name_span.clone())
        // Only a genuine predecessor in the canonical, schema-glob-first
        // order redefines — see `table.rs`'s identical guard.
        .filter(|existing| ctx.source_precedes(existing.source()));

    if let Some(existing) = existing {
        super::emit_duplicate_definition(
            ctx,
            stmt.name.span,
            &format!("`{}` on `{}`", stmt.name.node, stmt.table.node),
            &format!(
                "DEFINE INDEX OVERWRITE {} ON {}",
                stmt.name.node, stmt.table.node
            ),
            existing,
        );
    }

    for (text, span) in unknown_fields {
        let mut finding = surrealql_analyzer_diagnostics::catalog::finding(
            span,
            1002,
            format!(
                "`{}` has no field `{text}` (used by index `{}`)",
                stmt.table.node, stmt.name.node
            ),
        );
        if let Some(nearest) = crate::suggest::closest(&text, field_keys.iter().map(String::as_str))
        {
            finding = finding.with_help(format!("did you mean `{nearest}`?"));
        }
        finding = finding.with_related(
            table_name_span.clone(),
            format!("`{}` is defined here", stmt.table.node),
        );
        ctx.emit(finding);
    }

    // A full-text index tokenizes with a named analyzer. The engine accepts
    // `FULLTEXT ANALYZER ghost` at definition time (verified on 3.2.3: the
    // DEFINE returns NONE) and fails only when the index is first searched,
    // so nothing else catches the misspelling (1012).
    if let Some(analyzer) = &stmt.analyzer {
        let missing = ctx.schema().analyzer(&analyzer.node).is_none();
        if missing {
            let names: Vec<String> = ctx.schema().analyzers.keys().cloned().collect();
            let mut finding = surrealql_analyzer_diagnostics::catalog::finding(
                SourceSpan::new(ctx.source().clone(), analyzer.span),
                1012,
                format!(
                    "index `{}` tokenizes with `{}`, which is not a defined analyzer",
                    stmt.name.node, analyzer.node
                ),
            )
            .with_help(format!(
                "no `DEFINE ANALYZER {}` exists in the workspace",
                analyzer.node
            ));
            if let Some(nearest) =
                crate::suggest::closest(&analyzer.node, names.iter().map(String::as_str))
            {
                finding = finding.with_help(format!("did you mean `{nearest}`?"));
            }
            ctx.emit(finding);
        }
    }

    if let Some(existing) = duplicate {
        ctx.emit(
            surrealql_analyzer_diagnostics::catalog::finding(
                SourceSpan::new(ctx.source().clone(), stmt.name.span),
                1029,
                format!(
                    "index `{}` covers the same fields as `{existing}`",
                    stmt.name.node
                ),
            )
            .with_help("drop one — the duplicate index adds write cost without benefit"),
        );
    }

    Kind::None
}

/// 1033 — a `COUNT` index takes no `FIELDS`. Verified on 3.2.3: `DEFINE
/// INDEX icnt ON user FIELDS name COUNT` fails with "Cannot create a count
/// index with fields", while a bare `DEFINE INDEX icnt ON user COUNT`
/// defines fine. The grammar takes the combination — `CountClause` and
/// `FieldsColumnsClause` are independent repeated clauses, and the engine's
/// own restriction is a statement-level check on its parser, not a
/// context-free grammar rule — so this is what names the mistake instead of
/// the file collapsing into "Cannot create a count index with fields" with
/// no span, or (worse) analysis silently treating it as a normal index.
fn check_count_index_fields(ctx: &mut AnalysisContext<'_>, stmt: &ast::DefineIndex) {
    if stmt.kind != ast::IndexKind::Count {
        return;
    }
    let (Some(first), Some(last)) = (stmt.fields.first(), stmt.fields.last()) else {
        return;
    };
    let span = ByteRange::new(first.span.start(), last.span.end()).unwrap_or(first.span);
    ctx.emit(
        surrealql_analyzer_diagnostics::catalog::finding(
            SourceSpan::new(ctx.source().clone(), span),
            1033,
            format!(
                "index `{}` is COUNT and also names FIELDS — a count index takes no fields",
                stmt.name.node
            ),
        )
        .with_help(
            "SurrealDB fails this definition: \"Cannot create a count index with fields\" — drop FIELDS for an unconditional count, or drop COUNT for a normal index over these fields",
        ),
    );
}

/// The `REBUILD`/`REMOVE INDEX` reference contract (1012): the named index
/// must exist on the named table.
pub(crate) fn check_index_target(
    ctx: &mut AnalysisContext<'_>,
    index: &str,
    index_span: ByteRange,
    table: &str,
    table_span: ByteRange,
    statement: &str,
) {
    let source = ctx.source().clone();
    match ctx.schema().table(table) {
        None => ctx.emit(surrealql_analyzer_diagnostics::catalog::finding(
            SourceSpan::new(source, table_span),
            1012,
            format!("{statement} targets `{table}`, which is not a defined table"),
        )),
        Some(table_def) if !table_def.indexes.contains_key(index) => {
            ctx.emit(surrealql_analyzer_diagnostics::catalog::finding(
                SourceSpan::new(source, index_span),
                1012,
                format!("`{table}` has no index `{index}` ({statement})"),
            ));
        }
        Some(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::analysis::{analyze_query, Workspace};

    fn codes(query: &str) -> Vec<String> {
        let mut workspace = Workspace::default();
        analyze_query(&mut workspace, query)
            .diagnostics
            .iter()
            .map(|finding| finding.code().to_string())
            .collect()
    }

    #[test]
    fn a_full_text_index_naming_an_undefined_analyzer_is_1012() {
        let base = "DEFINE TABLE p SCHEMAFULL; DEFINE FIELD n ON p TYPE string; DEFINE ANALYZER simple TOKENIZERS blank;";
        for clause in [
            "FULLTEXT ANALYZER simpel BM25",
            "SEARCH ANALYZER simpel BM25",
        ] {
            let query = format!("{base} DEFINE INDEX ft ON p FIELDS n {clause};");
            assert!(
                codes(&query).contains(&"E1012".to_string()),
                "{clause}: {:?}",
                codes(&query)
            );
        }
        let defined =
            format!("{base} DEFINE INDEX ft ON p FIELDS n FULLTEXT ANALYZER simple BM25;");
        assert!(
            !codes(&defined).contains(&"E1012".to_string()),
            "{:?}",
            codes(&defined)
        );
    }

    #[test]
    fn a_count_index_naming_fields_is_1033() {
        let base = "DEFINE TABLE user SCHEMAFULL; DEFINE FIELD name ON user TYPE string;";
        let query = format!("{base} DEFINE INDEX icnt ON user FIELDS name COUNT;");
        assert!(
            codes(&query).contains(&"E1033".to_string()),
            "{:?}",
            codes(&query)
        );
        // Silent without FIELDS — an unconditional or `WHERE`-guarded count
        // index is exactly what COUNT is for.
        for ok in [
            format!("{base} DEFINE INDEX icnt ON user COUNT;"),
            format!("{base} DEFINE INDEX icnt ON user COUNT WHERE name != '';"),
            format!("{base} DEFINE INDEX ifields ON user FIELDS name;"),
        ] {
            assert!(
                !codes(&ok).contains(&"E1033".to_string()),
                "{ok}: {:?}",
                codes(&ok)
            );
        }
    }
}
