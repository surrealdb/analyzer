//! `set::map` function analysis: `set::map(set, closure) -> set`.
//!
//! The element kind of the result is the closure's return kind, inferred
//! with the closure's parameters bound to the call site's element kind (the
//! closure receives `[value, index]`). Mapping preserves the array length.

use surrealdb_types::Kind;
use surrealql_analyzer_syntax::ast;

use crate::analyzer::context::AnalysisContext;
use crate::analyzer::expression::infer::closure_return_kind;
use crate::analyzer::function::signature::{ParamKind, ReturnKind, Signature};
use crate::analyzer::function::{check_closure_arity, closure_arg};

/// The declared shape, for the catalog; the analyzer body below derives
/// the return kind itself rather than checking calls against this.
pub(crate) fn signature() -> Signature {
    Signature {
        min_args: 2,
        max_args: Some(2),
        arg_kinds: vec![ParamKind::Array, ParamKind::Closure],
        return_kind: ReturnKind::Fixed(Kind::Set(Box::new(Kind::Any), None)),
    }
}

pub(crate) fn analyze_set_map(
    ctx: &mut AnalysisContext<'_>,
    call: &ast::Call,
    args: &[Kind],
) -> Kind {
    let Some((Kind::Set(element, max_len), closure)) =
        args.first().cloned().zip(closure_arg(call, 1))
    else {
        if let Some(kind) = args.first() {
            crate::analyzer::function::check_argument_could_be(
                ctx,
                call,
                0,
                kind,
                |kind| matches!(kind, Kind::Array(_, _) | Kind::Set(_, _)),
                "an array or set",
            );
        }
        return Kind::Any;
    };
    check_closure_arity(ctx, call, closure, 2);

    let param_kinds = [(*element).clone(), Kind::Int];
    let mapped = closure_return_kind(closure, &param_kinds, ctx).unwrap_or(Kind::Any);
    crate::analyzer::expression::check::check_closure_with_param_kinds(ctx, closure, &param_kinds);
    Kind::Set(Box::new(mapped), max_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::test_support;

    #[test]
    fn without_a_closure_expression_the_result_is_unknown() {
        // Synthetic calls carry no argument expressions, so there is no
        // closure body to infer from.
        test_support::with_ctx(|ctx| {
            let call = test_support::synthetic_call("set::map");
            assert_eq!(
                analyze_set_map(ctx, &call, &[Kind::Set(Box::new(Kind::Int), None)]),
                Kind::Any
            );
        });
    }

    fn codes(query: &str) -> Vec<u16> {
        use surrealql_analyzer_diagnostics::Finding;
        use surrealql_analyzer_syntax::parse::parse_source;
        use surrealql_analyzer_syntax::source::SourceId;

        let parsed = parse_source(SourceId::new("fn:test"), query).expect("query parses");
        let lowered = surrealql_analyzer_syntax::lower::lower_first_expr(&parsed, "FunctionCall")
            .expect("function call node");
        let schema = crate::schema::SchemaIndex::default();
        let mut diagnostics: Vec<Finding> = Vec::new();
        let mut ctx = AnalysisContext::new(
            &schema,
            parsed.source_id().clone(),
            parsed.text(),
            &mut diagnostics,
        );
        crate::analyzer::expression::analyze_expr(&mut ctx, &lowered);
        diagnostics.iter().map(|f| f.code().number()).collect()
    }

    #[test]
    fn the_closures_body_is_checked_against_the_element_it_receives() {
        assert!(
            codes("RETURN set::map(<set> [1, 2], |$v| $v + 'a');").contains(&2004),
            "{:?}",
            codes("RETURN set::map(<set> [1, 2], |$v| $v + 'a');")
        );
        assert!(!codes("RETURN set::map(<set> [1, 2], |$v| $v + 1);").contains(&2004));
    }

    #[test]
    fn a_non_collection_receiver_is_5002() {
        assert!(codes("RETURN set::map(30, |$v| $v);").contains(&5002));
    }
}
