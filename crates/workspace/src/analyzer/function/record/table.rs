//! `record::table` function analysis: `record::table(record) -> table`.
//!
//! The argument must be a `record`; see `record::id` for the engine text and
//! why the check goes through
//! [`crate::analyzer::function::check_argument_could_be`] rather than
//! `arg_kinds`.

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

pub(crate) fn analyze_record_table(
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
            |kind| matches!(kind, Kind::Record(_)),
            "a `record`",
        );
    }
    apply(ctx, call, &signature(), args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::function::signature::evaluate;

    #[test]
    fn table_of_record_is_table() {
        assert_eq!(
            evaluate(&signature(), &[Kind::Record(Vec::new())]),
            Kind::Table(Vec::new())
        );
    }
}
