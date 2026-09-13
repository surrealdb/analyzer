//! `array::push` function analysis: `array::push(array, value) -> array`.
//!
//! `array::push`'s own return kind is `SameAsArg(0)` — the array's kind
//! unchanged — which is honest about the call itself (`array::push([1,2],
//! 'a')` returns `[1, 2, 'a']` at the call site with no error of any kind:
//! a bare array tolerates mixed elements). The failure verified on 3.2.3 is
//! one step removed: writing that mixed result into a `TYPE array<string>`
//! field is what fails ("Expected `string` but found `5` when coercing
//! element at index 1 of `array<string>`"). So this is checked directly here
//! against the array's OWN declared element kind, not by widening the return
//! kind (which would risk misreporting the *call's* result as narrower than
//! it is everywhere else this function's return is read) and not by
//! `arg_kinds`' generic per-argument check (`ParamKind::Any` on the pushed
//! value would only ever ask "is this NONE", never "does this match the
//! element kind" — the element the value fits or doesn't is the whole
//! contract, not whether the argument happens to be optional).

use surrealdb_types::Kind;
use surrealql_analyzer_syntax::ast;

use crate::analyzer::context::AnalysisContext;
use crate::analyzer::function::signature::{apply, ParamKind, ReturnKind, Signature};

/// The signature calls are checked against.
pub(crate) fn signature() -> Signature {
    Signature {
        min_args: 2,
        max_args: Some(2),
        arg_kinds: vec![ParamKind::Array, ParamKind::Any],
        return_kind: ReturnKind::SameAsArg(0),
    }
}

pub(crate) fn analyze_array_push(
    ctx: &mut AnalysisContext<'_>,
    call: &ast::Call,
    args: &[Kind],
) -> Kind {
    check_pushed_value_kind(ctx, call, args);
    apply(ctx, call, &signature(), args)
}

/// 2001 — the value pushed must fit the array's own declared element kind.
/// Silent on an unknown element kind (`array<any>`, an unresolved column) and
/// on a pushed value that might still fit (`option<string>` into
/// `array<string>` — the value could be a string at runtime).
fn check_pushed_value_kind(ctx: &mut AnalysisContext<'_>, call: &ast::Call, args: &[Kind]) {
    let (Some(Kind::Array(element, _)), Some(value)) = (args.first(), args.get(1)) else {
        return;
    };
    if matches!(element.as_ref(), Kind::Any) {
        return;
    }
    if crate::kinds::could_satisfy(value, &|kind| {
        crate::kinds::kind_is_assignable_to(kind, element)
    }) {
        return;
    }
    let Some(arg_expr) = call.args.get(1) else {
        return;
    };
    let span =
        surrealql_analyzer_syntax::span::SourceSpan::new(ctx.source().clone(), arg_expr.span);
    let declared = crate::render_kind(element);
    let actual = crate::render::render_offending(value, Some(element));
    ctx.emit(
        surrealql_analyzer_diagnostics::catalog::finding(
            span,
            2001,
            format!("this array holds `{declared}`, but the pushed value is `{actual}`"),
        )
        .with_help(format!(
            "SurrealDB fails the write: \"Expected `{declared}` but found ... when coercing element at index N of `array<{declared}>`\""
        )),
    );
}
