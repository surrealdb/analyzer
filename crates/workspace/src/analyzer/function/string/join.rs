//! `string::join` function analysis: `string::join(separator, ...any) -> string`.
//!
//! Variadic: the first argument is the separator, the remaining arguments are
//! joined by it. Every position is `any` — the engine stringifies whatever it
//! is handed, on 3.2.3 `string::join(',', 1, 2)` → `'1,2'`,
//! `string::join(',', [1, 2])` → `'[1, 2]'`, and even the separator coerces:
//! `string::join(1, 'a', 'b')` → `'a1b'`. Declaring the positions
//! `Exact(string)` made joining numbers — which real code does routinely — two
//! build-breaking 5002s. `string::concat` next door has always been spelled
//! this way.

use surrealdb_types::Kind;
use surrealql_analyzer_syntax::ast;

use crate::analyzer::context::AnalysisContext;
use crate::analyzer::function::signature::{apply, ParamKind, ReturnKind, Signature};

/// The signature calls are checked against.
pub(crate) fn signature() -> Signature {
    Signature {
        min_args: 1,
        max_args: None,
        arg_kinds: vec![ParamKind::Any],
        return_kind: ReturnKind::Fixed(Kind::String),
    }
}

pub(crate) fn analyze_string_join(
    ctx: &mut AnalysisContext<'_>,
    call: &ast::Call,
    args: &[Kind],
) -> Kind {
    apply(ctx, call, &signature(), args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::function::signature::evaluate;

    #[test]
    fn returns_string_for_variadic_strings() {
        assert_eq!(
            evaluate(&signature(), &[Kind::String, Kind::String, Kind::String]),
            Kind::String
        );
    }

    #[test]
    fn returns_string_for_variadic_non_strings() {
        assert_eq!(
            evaluate(&signature(), &[Kind::String, Kind::Int, Kind::Int]),
            Kind::String
        );
    }
}
