//! `UPDATE` statement analysis.
//!
//! Owns UPDATE-specific analysis; carries its own `WHERE` clause, whose
//! UPDATE-specific invariants belong here.

use surrealdb_types::Kind;
use surrealql_analyzer_syntax::ast;

use crate::analyzer::context::AnalysisContext;
use crate::analyzer::data::mutation;

pub(crate) fn analyze_update(ctx: &mut AnalysisContext<'_>, stmt: &ast::UpdateStmt) -> Kind {
    update_response_kind(stmt, ctx)
}

pub(crate) fn update_response_kind(stmt: &ast::UpdateStmt, ctx: &mut AnalysisContext<'_>) -> Kind {
    mutation::check_only_on_table(ctx, stmt.only, stmt.targets.first());
    mutation::check_payload_id_against_target(ctx, &stmt.targets, stmt.data.as_ref());
    mutation::check_whole_table_write(ctx, stmt.targets.first(), stmt.where_clause.as_ref());
    let table_name = mutation::target_table_name(ctx, stmt.targets.first());
    let table_hint = table_name.clone();
    mutation::analyze_expression_positions_for(
        ctx,
        stmt.data.as_ref(),
        stmt.where_clause.as_ref(),
        table_hint.as_deref(),
        false,
    );
    let Some(table_name) = table_name else {
        return Kind::Any;
    };
    let Some(table) = ctx.schema().tables.get(&table_name) else {
        if let Some(target) = stmt.targets.first() {
            crate::analyzer::data::check_table_reference(ctx, &table_name, target.span);
        }
        return Kind::Any;
    };

    mutation::response_kind_for_target(stmt.only, stmt.ret.as_ref(), table, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SchemaIndex;
    use crate::statement_env::StatementEnv;
    use surrealql_analyzer_syntax::parse::parse_source;
    use surrealql_analyzer_syntax::source::SourceId;

    use crate::schema::extract_schema;

    fn analyze(schema: &SchemaIndex, query: &str) -> Kind {
        analyze_with_diagnostics(schema, query).0
    }

    fn analyze_with_diagnostics(
        schema: &SchemaIndex,
        query: &str,
    ) -> (Kind, Vec<surrealql_analyzer_diagnostics::Finding>) {
        let parsed = parse_source(SourceId::new("query"), query).expect("query should parse");
        let ast::Statement::Update(stmt) =
            surrealql_analyzer_syntax::lower::lower_first_statement(&parsed, "UpdateStatement")
                .expect("update statement exists")
                .node
        else {
            panic!("expected update statement");
        };
        let env = StatementEnv::default();
        let mut diagnostics: Vec<surrealql_analyzer_diagnostics::Finding> = Vec::new();
        let kind = {
            let mut ctx = AnalysisContext::scoped(
                schema,
                parsed.source_id().clone(),
                parsed.text(),
                &mut diagnostics,
                env,
                None,
            );
            update_response_kind(&stmt, &mut ctx)
        };
        (kind, diagnostics)
    }

    fn person_schema() -> SchemaIndex {
        let parsed = parse_source(
            SourceId::new("schema"),
            "DEFINE TABLE person SCHEMAFULL;\n\
             DEFINE FIELD name ON person TYPE string;\n\
             DEFINE FIELD active ON person TYPE bool DEFAULT true;",
        )
        .expect("schema should parse");
        extract_schema(&[parsed]).schema
    }

    fn missing_fields(diagnostics: &[surrealql_analyzer_diagnostics::Finding]) -> usize {
        diagnostics
            .iter()
            .filter(|finding| finding.code().number() == 2034)
            .count()
    }

    #[test]
    fn replace_omitting_a_defaulted_field_is_2034() {
        // REPLACE never re-applies DEFAULT, so `active` (DEFAULT true) is
        // still required here even though a CREATE could omit it.
        let schema = person_schema();
        let (_, diagnostics) =
            analyze_with_diagnostics(&schema, "UPDATE person:1 REPLACE { name: 'A' };");
        assert_eq!(missing_fields(&diagnostics), 1, "{diagnostics:?}");
    }

    #[test]
    fn replace_providing_every_field_stays_silent() {
        let schema = person_schema();
        let (_, diagnostics) = analyze_with_diagnostics(
            &schema,
            "UPDATE person:1 REPLACE { name: 'A', active: false };",
        );
        assert_eq!(missing_fields(&diagnostics), 0, "{diagnostics:?}");
    }

    #[test]
    fn replace_omitting_a_value_or_computed_field_stays_silent() {
        // Unlike a plain DEFAULT, VALUE/COMPUTED recompute unconditionally
        // on every write — verified on 3.2.3 for both a COMPUTED field and a
        // VALUE field that ignores $value, omitted from a REPLACE payload.
        let schema_parsed = parse_source(
            SourceId::new("schema"),
            "DEFINE TABLE person SCHEMAFULL;\n\
             DEFINE FIELD name ON person TYPE string;\n\
             DEFINE FIELD stamp ON person TYPE datetime VALUE time::now();\n\
             DEFINE FIELD tot ON person COMPUTED 1 + 1;",
        )
        .expect("schema should parse");
        let schema = extract_schema(&[schema_parsed]).schema;
        let (_, diagnostics) =
            analyze_with_diagnostics(&schema, "UPDATE person:1 REPLACE { name: 'A' };");
        assert_eq!(missing_fields(&diagnostics), 0, "{diagnostics:?}");
    }

    #[test]
    fn content_omitting_a_defaulted_field_stays_silent() {
        // Unlike REPLACE, CONTENT's DEFAULT still applies at creation — this
        // is UPDATE (not CREATE) so 2034 does not run there at all, but the
        // point stands: CONTENT is not REPLACE and must not be checked as one.
        let schema = person_schema();
        let (_, diagnostics) =
            analyze_with_diagnostics(&schema, "UPDATE person:1 CONTENT { name: 'A' };");
        assert_eq!(missing_fields(&diagnostics), 0, "{diagnostics:?}");
    }

    fn wrote_schema() -> SchemaIndex {
        let parsed = parse_source(
            SourceId::new("schema"),
            "DEFINE TABLE account SCHEMAFULL;\n\
             DEFINE TABLE wrote TYPE RELATION FROM account TO account SCHEMAFULL;\n\
             DEFINE FIELD weight ON wrote TYPE float;",
        )
        .expect("schema should parse");
        extract_schema(&[parsed]).schema
    }

    fn discarded_endpoint_writes(diagnostics: &[surrealql_analyzer_diagnostics::Finding]) -> usize {
        diagnostics
            .iter()
            .filter(|finding| finding.code().number() == 2039)
            .count()
    }

    #[test]
    fn set_on_in_or_out_of_an_existing_edge_is_2039() {
        let schema = wrote_schema();
        for query in [
            "UPDATE wrote SET in = account:2;",
            "UPDATE wrote SET out = account:2;",
            "UPDATE wrote:1 SET in = account:2, weight = 2.0;",
        ] {
            let (_, diagnostics) = analyze_with_diagnostics(&schema, query);
            assert_eq!(
                discarded_endpoint_writes(&diagnostics),
                1,
                "{query}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn content_or_merge_naming_in_or_out_of_an_existing_edge_is_2039() {
        let schema = wrote_schema();
        for query in [
            "UPDATE wrote:1 CONTENT { in: account:2, out: account:3, weight: 1.0 };",
            "UPDATE wrote:1 MERGE { in: account:2 };",
        ] {
            let (_, diagnostics) = analyze_with_diagnostics(&schema, query);
            assert!(
                discarded_endpoint_writes(&diagnostics) >= 1,
                "{query}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn a_write_that_never_names_in_or_out_stays_silent() {
        let schema = wrote_schema();
        let (_, diagnostics) =
            analyze_with_diagnostics(&schema, "UPDATE wrote:1 SET weight = 2.0;");
        assert_eq!(
            discarded_endpoint_writes(&diagnostics),
            0,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn in_or_out_on_a_plain_table_is_not_2039() {
        // `in`/`out` are ordinary field names on a table that is not a
        // relation — nothing is discarded there.
        let schema_parsed = parse_source(
            SourceId::new("schema"),
            "DEFINE TABLE plain SCHEMAFULL;\nDEFINE FIELD in ON plain TYPE string;",
        )
        .expect("schema should parse");
        let schema = extract_schema(&[schema_parsed]).schema;
        let (_, diagnostics) = analyze_with_diagnostics(&schema, "UPDATE plain:1 SET in = 'x';");
        assert_eq!(
            discarded_endpoint_writes(&diagnostics),
            0,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn infers_array_of_full_table_rows_by_default() {
        let schema_parsed = parse_source(
            SourceId::new("schema"),
            "DEFINE TABLE person SCHEMAFULL;\nDEFINE FIELD name ON person TYPE string;",
        )
        .expect("schema should parse");
        let schema = extract_schema(&[schema_parsed]).schema;

        let kind = analyze(&schema, "UPDATE person SET name = 'A';");

        let Kind::Array(element, None) = kind else {
            panic!("expected unbounded array kind, got {kind:?}");
        };
        let Kind::Literal(surrealdb_types::KindLiteral::Object(fields)) = *element else {
            panic!("expected object literal element");
        };
        assert_eq!(fields["name"], Kind::String);
    }

    #[test]
    fn unknown_target_table_is_poison() {
        let schema = SchemaIndex::default();

        let kind = analyze(&schema, "UPDATE ghost SET name = 'A';");

        assert_eq!(kind, Kind::Any);
    }
}
