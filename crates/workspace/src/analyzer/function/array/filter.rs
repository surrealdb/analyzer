//! `array::filter` function analysis: `array::filter(array, closure) -> array`.
//!
//! Filtering preserves the input array's element kind and drops its length
//! bound — and, when the predicate is a closure, *narrows* that element by
//! whatever the predicate proves.
//!
//! That last part is the same fact a `WHERE` states about a SELECT's surviving
//! rows, written one abstraction down: every surviving element satisfies the
//! predicate, so the predicate is a positive guard over the element. The two
//! spellings had different answers only because the row side had a recognizer
//! and this side had nothing:
//!
//! ```text
//! (SELECT name, email FROM user WHERE email != NONE)      -> array<{email: string, …}>
//! (SELECT name, email FROM user).filter(|$r| $r.email != NONE)
//!                                                          -> array<{email: option<string>, …}>
//! ```
//!
//! Now both go through `guard_of` + `Facts`, so they agree — and so do the
//! spellings of the predicate that no recognizer ever accepted
//! (`!($r.email = NONE)`, `type::is_string($r.email)`, a parenthesized any of
//! them).

use surrealdb_types::Kind;
use surrealql_analyzer_syntax::ast;

use crate::analyzer::context::AnalysisContext;
use crate::analyzer::facts::{guard_of, refined_under, PlaceRoot, RootedKind};
use crate::analyzer::function::signature::{ParamKind, ReturnKind, Signature};

/// The declared shape, for the catalog; the analyzer body below derives
/// the return kind itself rather than checking calls against this.
pub(crate) fn signature() -> Signature {
    Signature {
        min_args: 2,
        max_args: Some(2),
        arg_kinds: vec![ParamKind::Array, ParamKind::Closure],
        return_kind: ReturnKind::Fixed(Kind::Array(Box::new(Kind::Any), None)),
    }
}

pub(crate) fn analyze_array_filter(
    ctx: &mut AnalysisContext<'_>,
    call: &ast::Call,
    args: &[Kind],
) -> Kind {
    let [Kind::Array(element, _), ..] = args else {
        // 5002: the receiver must be a collection — verified on 3.2.3 the
        // same way `array::map` is.
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
    check_predicate_closure_body(ctx, call, element);
    let element = filtered_element(ctx, call, element.as_ref().clone());
    Kind::Array(Box::new(element), None)
}

/// The predicate closure's body was never checked against the element it
/// receives — [`filtered_element`] only reads what the guard *proves*, never
/// what it *contains*, so `tags.filter(|$t| $t > 3)` over `array<string>`
/// silently narrowed nothing rather than reporting the comparison (2004)
/// that never can hold.
fn check_predicate_closure_body(ctx: &mut AnalysisContext<'_>, call: &ast::Call, element: &Kind) {
    let Some(predicate) = call.args.get(1) else {
        return;
    };
    let ast::Expr::Closure(closure) = &predicate.node else {
        return;
    };
    let [(_, _)] = closure.params.as_slice() else {
        return;
    };
    crate::analyzer::expression::check::check_closure_with_param_kinds(
        ctx,
        closure,
        std::slice::from_ref(element),
    );
}

/// The element kind of the surviving elements: the input element, refined by
/// what a single-parameter predicate closure proves about its parameter.
///
/// Widening is the safe direction, so every step that cannot be proven returns
/// the element unchanged: a predicate that is not a one-parameter closure, a
/// guard that claims nothing, a claim about a place the element does not carry.
fn filtered_element(ctx: &mut AnalysisContext<'_>, call: &ast::Call, element: Kind) -> Kind {
    let Some(predicate) = call.args.get(1) else {
        return element;
    };
    let ast::Expr::Closure(closure) = &predicate.node else {
        return element;
    };
    // Exactly one parameter: `array::filter` calls the predicate with the
    // element alone, and a second parameter would name something this scope
    // does not bind.
    let [(param, _)] = closure.params.as_slice() else {
        return element;
    };
    let root = PlaceRoot::Param(param.node.clone());
    let guard = guard_of(&closure.body.node, true, Some(ctx.env()));
    let facts = guard.facts(&RootedKind {
        root: root.clone(),
        kind: &element,
    });
    refined_under(element, &root, &facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::test_support;
    use surrealql_analyzer_diagnostics::Finding;
    use surrealql_analyzer_syntax::parse::parse_source;
    use surrealql_analyzer_syntax::source::SourceId;

    #[test]
    fn preserves_the_input_array_element_kind() {
        test_support::with_ctx(|ctx| {
            let call = test_support::synthetic_call("array::filter");
            assert_eq!(
                analyze_array_filter(
                    ctx,
                    &call,
                    &[Kind::Array(Box::new(Kind::Int), Some(3)), Kind::Any]
                ),
                Kind::Array(Box::new(Kind::Int), None)
            );
        });
    }

    fn codes(query: &str) -> Vec<u16> {
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
    fn the_predicates_body_is_checked_against_the_element_it_receives() {
        // `$t`'s kind at the call site is `string` — comparing it to `3`
        // (int) is 2004, not a comparison that can ever hold.
        assert!(codes("RETURN array::filter(['a', 'b'], |$t| $t > 3);").contains(&2004));
        assert!(!codes("RETURN array::filter(['a', 'b'], |$t| $t = 'a');").contains(&2004));
    }

    #[test]
    fn a_non_collection_receiver_is_5002() {
        assert!(codes("RETURN array::filter(30, |$v| $v > 0);").contains(&5002));
    }
}
