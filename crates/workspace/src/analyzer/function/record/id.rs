//! `record::id` function analysis: `record::id(record) -> any`.
//!
//! A record ID can be one of many shapes (string, int, uuid, array, object,
//! range, ...), so the ID kind is genuinely not determinable statically —
//! `Any` here is an honest "unknown", not an unimplemented gap.
//!
//! The argument itself must be a `record`: verified on 3.2.3, `record::id('ann')`
//! fails with "Incorrect arguments for function `record::id()`. Argument 1 was
//! the wrong type. Expected `record` but found `'ann'`" — `record::tb`/
//! `record::table` answer the same way for the same reason. Checked through
//! [`crate::analyzer::function::check_argument_could_be`], not a plain
//! `ParamKind::Exact`: an `option<record<t>>` argument (a field or param that
//! might be `NONE`) only fails when it actually is `NONE` at runtime, so it is
//! not a call that is always wrong, and `arg_kinds`' union-source rule would
//! have flagged it as one.

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
        return_kind: ReturnKind::Fixed(Kind::Any),
    }
}

pub(crate) fn analyze_record_id(
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
    use crate::analysis::{analyze_query, Workspace};

    fn fires(query: &str, code: &str) -> bool {
        let mut workspace = Workspace::default();
        analyze_query(&mut workspace, query)
            .diagnostics
            .iter()
            .any(|finding| finding.code().to_string() == code)
    }

    #[test]
    fn a_non_record_argument_is_5002() {
        assert!(fires("RETURN record::id('ann');", "E5002"));
        assert!(fires("RETURN record::tb('ann');", "E5002"));
        assert!(fires("RETURN record::table('ann');", "E5002"));
    }

    #[test]
    fn a_record_argument_stays_silent() {
        assert!(!fires("RETURN record::id(person:1);", "E5002"));
        assert!(!fires("RETURN record::tb(person:1);", "E5002"));
        assert!(!fires("RETURN record::table(person:1);", "E5002"));
    }
}
