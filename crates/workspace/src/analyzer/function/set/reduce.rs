//! `set::reduce` function analysis:
//! `array::reduce(set, closure) -> accumulated`.
//!
//! The result is the closure's return kind, inferred with its parameters
//! bound to `[accumulator, value, index]` at the call site.

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
        return_kind: ReturnKind::Fixed(Kind::Any),
    }
}

pub(crate) fn analyze_set_reduce(
    ctx: &mut AnalysisContext<'_>,
    call: &ast::Call,
    args: &[Kind],
) -> Kind {
    let element = match args.first() {
        Some(Kind::Array(element, _) | Kind::Set(element, _)) => (**element).clone(),
        Some(kind) => {
            crate::analyzer::function::check_argument_could_be(
                ctx,
                call,
                0,
                kind,
                |kind| matches!(kind, Kind::Array(_, _) | Kind::Set(_, _)),
                "an array or set",
            );
            return Kind::Any;
        }
        None => return Kind::Any,
    };
    let (accumulator, closure) = (element.clone(), closure_arg(call, 1));
    let Some(closure) = closure else {
        return Kind::Any;
    };
    check_closure_arity(ctx, call, closure, 3);

    let param_kinds = [accumulator, element, Kind::Int];
    let result = closure_return_kind(closure, &param_kinds, ctx).unwrap_or(Kind::Any);
    crate::analyzer::expression::check::check_closure_with_param_kinds(ctx, closure, &param_kinds);
    result
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
            let call = test_support::synthetic_call("set::reduce");
            assert_eq!(
                analyze_set_reduce(ctx, &call, &[Kind::Set(Box::new(Kind::Int), None)]),
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
    fn the_closures_body_is_checked_against_the_accumulator_and_element() {
        assert!(
            !codes("RETURN set::reduce(<set> ['a', 'b'], |$acc, $v| $acc + $v);").contains(&2004)
        );
        assert!(codes("RETURN set::reduce(<set> [1, 2], |$acc, $v| $acc + 'x');").contains(&2004));
    }

    #[test]
    fn a_non_collection_receiver_is_5002() {
        assert!(codes("RETURN set::reduce(30, |$acc, $v| $acc);").contains(&5002));
    }
}
