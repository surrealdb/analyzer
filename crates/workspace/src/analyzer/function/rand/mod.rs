//! `rand` function family: every built-in it dispatches, with its analyzer.

use surrealql_analyzer_syntax::ast;

use crate::analyzer::context::AnalysisContext;
use crate::analyzer::function::BuiltinEntry;

pub mod bool;
pub mod duration;
pub mod r#enum;
pub mod float;
pub mod id;
pub mod int;
pub mod string;
pub mod time;
pub mod ulid;
// The bare `rand()` builtin has no path segment after its family, so the
// file mirroring it is `rand/rand.rs` — the naming rule every other leaf
// follows, not an accidentally nested module.
#[allow(clippy::module_inception)]
pub mod rand;
pub mod uuid;
pub mod uuid_v4;
pub mod uuid_v7;

/// Every `rand::` built-in the analyzer resolves, in dispatch order.
pub(crate) static CATALOG: &[BuiltinEntry] = &[
    BuiltinEntry::new(
        "rand::bool",
        "A random boolean.",
        bool::signature,
        bool::analyze_rand_bool,
    ),
    BuiltinEntry::new(
        "rand::duration",
        "A random duration between two bounds.",
        duration::signature,
        duration::analyze_rand_duration,
    ),
    BuiltinEntry::new(
        "rand::enum",
        "One of the given values, chosen at random.",
        r#enum::signature,
        r#enum::analyze_rand_enum,
    ),
    BuiltinEntry::new(
        "rand::float",
        "A random float, optionally between two bounds.",
        float::signature,
        float::analyze_rand_float,
    ),
    BuiltinEntry::new(
        "rand::id",
        "A random record id, or one for the given table.",
        id::signature,
        id::analyze_rand_id,
    ),
    BuiltinEntry::new(
        "rand::int",
        "A random integer, optionally between two bounds.",
        int::signature,
        int::analyze_rand_int,
    ),
    BuiltinEntry::new(
        "rand::string",
        "A random string, of an optional length or length range.",
        string::signature,
        string::analyze_rand_string,
    ),
    BuiltinEntry::new(
        "rand::time",
        "A random datetime, optionally between two bounds.",
        time::signature,
        time::analyze_rand_time,
    ),
    BuiltinEntry::new(
        "rand::ulid",
        "A random ULID, optionally seeded from a datetime.",
        ulid::signature,
        ulid::analyze_rand_ulid,
    ),
    BuiltinEntry::new(
        "rand::uuid",
        "A random UUID, optionally seeded from a datetime.",
        uuid::signature,
        uuid::analyze_rand_uuid,
    ),
    BuiltinEntry::new(
        "rand",
        "A random float between 0 and 1.",
        rand::signature,
        rand::analyze_rand_rand,
    ),
    BuiltinEntry::new(
        "rand::uuid::v4",
        "A random UUID v4.",
        uuid_v4::signature,
        uuid_v4::analyze_rand_uuid_v4,
    ),
    BuiltinEntry::new(
        "rand::uuid::v7",
        "A random UUID v7, optionally for a datetime.",
        uuid_v7::signature,
        uuid_v7::analyze_rand_uuid_v7,
    ),
];

/// 5002 for a `rand::` generator whose bounds must come as a *pair*.
///
/// `min_args`/`max_args` describe a contiguous range, and these four
/// generators do not have one: 3.2.3 takes both bounds or neither, and says
/// so per function —
///
/// ```text
/// rand::int(1)      -> Incorrect arguments for function rand::int(). Expected 0 or 2 arguments
/// rand::float(1)    -> … rand::float(). Expected 0 or 2 arguments
/// rand::time(1)     -> … rand::time(). Expected 0 or 2 arguments
/// ```
///
/// (`rand::duration` needs the same fix and does not need this check:
/// 3.2.3 requires *both* bounds there, which the signature's own range
/// expresses.)
///
/// Modelled as `0..=2`, the range caught `rand::int(1, 2, 3)` and let through
/// the one arity anybody writes by accident: `rand::int(100)`, read as "a
/// number up to 100". This is the odd-one-out case rather than a general
/// arity feature because it is one — `rand::string(5)` legitimately takes a
/// single argument, so the rule has to be per function.
///
/// Arities outside the signature's own range are left to `apply`, which
/// reports them the ordinary way; this only ever adds the gap in the middle.
pub(super) fn check_bounds_come_in_pairs(ctx: &mut AnalysisContext<'_>, call: &ast::Call) {
    if call.args.len() != 1 {
        return;
    }
    let span =
        surrealql_analyzer_syntax::span::SourceSpan::new(ctx.source().clone(), call.path.span);
    ctx.emit(
        surrealql_analyzer_diagnostics::catalog::finding(
            span,
            5002,
            format!(
                "`{}` takes 0 or 2 arguments, but this call passes 1 — its bounds come as a pair",
                call.path.node
            ),
        )
        .with_help(format!(
            "SurrealDB fails the call: \"Incorrect arguments for function {}(). Expected 0 or 2 arguments\"",
            call.path.node
        )),
    );
}

#[cfg(test)]
mod tests {
    use crate::analysis::{analyze_query, Workspace};

    fn codes(query: &str) -> Vec<String> {
        let mut workspace = Workspace::default();
        analyze_query(&mut workspace, query)
            .diagnostics
            .iter()
            .map(|finding| finding.code().to_string())
            .collect()
    }

    fn fires(query: &str, code: &str) -> bool {
        codes(query).iter().any(|c| c == code)
    }

    #[test]
    fn one_argument_is_5002_for_int_float_time() {
        for name in ["int", "float", "time"] {
            let one = format!("RETURN rand::{name}(1);");
            assert!(fires(&one, "E5002"), "{one}: {:?}", codes(&one));
            let zero = format!("RETURN rand::{name}();");
            assert!(!fires(&zero, "E5002"), "{zero}: {:?}", codes(&zero));
        }
        // Two bounds is the paired form; still legal.
        assert!(!fires("RETURN rand::int(1, 100);", "E5002"));
        assert!(!fires("RETURN rand::float(0.0, 1.0);", "E5002"));
    }

    #[test]
    fn one_argument_stays_legal_for_rand_string() {
        // The odd-one-out this check must not touch: `rand::string(5)` is a
        // genuinely unary call.
        assert!(!fires("RETURN rand::string(5);", "E5002"));
    }

    #[test]
    fn rand_duration_requires_both_bounds() {
        assert!(fires("RETURN rand::duration();", "E5002"));
        assert!(fires("RETURN rand::duration(1s);", "E5002"));
        assert!(!fires("RETURN rand::duration(1s, 2s);", "E5002"));
    }
}
