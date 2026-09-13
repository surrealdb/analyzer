//! `type::table` function analysis: `type::table(any) -> table`.
//!
//! The result is a table *value*, not a string — `type::of(type::table('person'))`
//! is `'table'` on 3.2.3 — so a constant argument makes this the same kind a
//! bare table name in the source infers (`Kind::Table(vec![person])`), and a
//! runtime argument leaves the table unconstrained.
//!
//! The argument is a `string` OR a `record` — verified on 3.2.3,
//! `type::table(person:1)` succeeds and answers `person` (the record's own
//! table), while `type::table(30)` and `type::table(true)` both fail with
//! "Found 30 for the Record ID but this is not a valid table name". Checked
//! through [`crate::analyzer::function::check_argument_could_be`] rather than
//! `arg_kinds`, the same way `record::id` is: an `option<record<t>>` or
//! `option<string>` argument only fails when it actually is `NONE`, and is
//! not a call that is always wrong.

use surrealdb_types::Kind;
use surrealql_analyzer_syntax::ast;

use crate::analyzer::context::AnalysisContext;
use crate::analyzer::function::signature::{apply, ParamKind, ReturnKind, Signature};

/// The signature calls are checked against.
pub(crate) fn signature() -> Signature {
    Signature {
        min_args: 1,
        max_args: Some(1),
        arg_kinds: vec![ParamKind::Any],
        return_kind: ReturnKind::Fixed(Kind::Table(Vec::new())),
    }
}

pub(crate) fn analyze_type_table(
    ctx: &mut AnalysisContext<'_>,
    call: &ast::Call,
    args: &[Kind],
) -> Kind {
    if let Some(kind) = args.first() {
        crate::analyzer::function::check_argument_could_be(
            ctx,
            call,
            0,
            kind,
            |kind| matches!(kind, Kind::String | Kind::Record(_)),
            "a `string` or `record`",
        );
    }
    let tables = super::constant_table_arg(ctx, call, 0)
        .map(|table| vec![table])
        .unwrap_or_default();
    let mut signature = signature();
    signature.return_kind = ReturnKind::Fixed(Kind::Table(tables));
    apply(ctx, call, &signature, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{analyze_query, Workspace};

    fn kind_of(query: &str) -> Option<Kind> {
        let mut workspace = Workspace::default();
        analyze_query(&mut workspace, query).response_kind
    }

    #[test]
    fn a_constant_argument_names_the_table() {
        assert_eq!(
            kind_of("RETURN type::table('person');"),
            Some(Kind::Table(vec!["person".into()]))
        );
    }

    #[test]
    fn a_runtime_argument_stays_unconstrained() {
        assert_eq!(
            kind_of("RETURN type::table($name);"),
            Some(Kind::Table(Vec::new()))
        );
    }

    fn fires(query: &str, code: &str) -> bool {
        let mut workspace = Workspace::default();
        analyze_query(&mut workspace, query)
            .diagnostics
            .iter()
            .any(|finding| finding.code().to_string() == code)
    }

    #[test]
    fn a_non_string_non_record_argument_is_5002() {
        assert!(fires("RETURN type::table(30);", "E5002"));
        assert!(fires("RETURN type::table(true);", "E5002"));
    }

    #[test]
    fn a_record_argument_stays_silent() {
        // `type::table(person:1)` is legal on 3.2.3 — it answers the
        // record's own table.
        assert!(!fires("RETURN type::table(person:1);", "E5002"));
    }

    #[test]
    fn an_optional_record_argument_stays_silent() {
        // `$o` might be NONE at runtime, but is not a call that is *always*
        // wrong — flagging it would conflate "might fail" with "can never
        // succeed".
        let schema = "DEFINE TABLE person SCHEMAFULL; DEFINE TABLE document SCHEMAFULL;\n\
             DEFINE FIELD owner ON document TYPE option<record<person>>;";
        let query = format!(
            "{schema} LET $o = (SELECT VALUE owner FROM ONLY document LIMIT 1); RETURN type::table($o);"
        );
        assert!(!fires(&query, "E5002"), "{:?}", {
            let mut workspace = Workspace::default();
            analyze_query(&mut workspace, &query)
                .diagnostics
                .iter()
                .map(|f| f.code().to_string())
                .collect::<Vec<_>>()
        });
    }
}
