//! `session::db` function analysis: `session::db() -> option<string>`.

use surrealdb_types::Kind;
use surrealql_analyzer_syntax::ast;

use crate::analyzer::context::AnalysisContext;
use crate::analyzer::function::signature::{apply, ReturnKind, Signature};

/// The signature calls are checked against.
///
/// `max_args: None` because the engine does: `RETURN session::db(1);` and
/// `RETURN session::db(1, 2, 3);` both answer `'t'` on 3.2.3, same as the
/// bare call — every argument is ignored, not rejected.
pub(crate) fn signature() -> Signature {
    Signature {
        min_args: 0,
        max_args: None,
        arg_kinds: vec![],
        return_kind: ReturnKind::Fixed(Kind::option(Kind::String)),
    }
}

pub(crate) fn analyze_session_db(
    ctx: &mut AnalysisContext<'_>,
    call: &ast::Call,
    args: &[Kind],
) -> Kind {
    apply(ctx, call, &signature(), args)
}
