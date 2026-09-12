//! The shapes the engine accepts that the grammar used to refuse.
//!
//! The mirror of `engine_refused_syntax.rs`, and the more costly direction of
//! the two. A parse error is fatal to the whole source: one of these in a
//! file cost the user every other finding in it, and told them their working
//! SurrealQL was a syntax error. Each case below runs on a live SurrealDB
//! 3.2.3 — either cleanly, or with a *specific* runtime error that the
//! analyzer already has a code for and could never reach while its own parser
//! rejected the statement first.
//!
//! So each test asserts two things: that the statement parses (no `S`-category
//! finding), and what the analyzer says about it now that it does — silence
//! where the engine is happy, and the cataloged code where the engine is not.

mod support;

use surrealql_analyzer_diagnostics::Finding;
use surrealql_analyzer_syntax::source::SourceId;
use surrealql_analyzer_workspace::{analyze_workspace, Workspace};

const SCHEMA: &str = "\
DEFINE TABLE person SCHEMAFULL;
DEFINE FIELD name ON person TYPE string;
DEFINE FIELD tags ON person TYPE array<string>;
DEFINE FIELD meta ON person TYPE object;
DEFINE FIELD meta.score ON person TYPE int;
";

/// Analyzes one query against `SCHEMA` and returns the findings on it.
fn findings(query: &str) -> Vec<Finding> {
    let mut workspace =
        Workspace::new(surrealql_analyzer_workspace::config::WorkspaceConfig::default());
    workspace.add_virtual_source("schema".into(), SCHEMA.into());
    let source: SourceId = workspace.add_virtual_source("query".into(), query.into());
    analyze_workspace(&workspace)
        .diagnostics
        .into_iter()
        .filter(|finding| finding.span().source() == &source)
        .collect()
}

/// The one finding with `code`, or a panic naming what was found instead.
fn only(query: &str, code: &str) -> Finding {
    let found = findings(query);
    support::assert_no_syntax_findings(&found);
    let matching: Vec<&Finding> = found
        .iter()
        .filter(|finding| finding.code().to_string() == code)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "`{query}` should raise exactly one {code}; got {:?}",
        found
            .iter()
            .map(|finding| (finding.code().to_string(), finding.message().to_string()))
            .collect::<Vec<_>>()
    );
    matching[0].clone()
}

/// `query` parses and raises nothing.
fn silent(query: &str) {
    let found = findings(query);
    support::assert_no_syntax_findings(&found);
    assert!(
        found.is_empty(),
        "`{query}` should raise nothing; got {:?}",
        found
            .iter()
            .map(|finding| (finding.code().to_string(), finding.message().to_string()))
            .collect::<Vec<_>>()
    );
}

/// `THROW` is an expression in SurrealQL, and the grammar admitted it only as
/// a statement — so SurrealKit's own read-only-table fixture was two `S0001`s,
/// with the spans landing mid-string (`'"Read'`, `'customer"'`) where the
/// parser resynced at the hyphen.
///
/// 3.2.3 defines both of these without complaint (`INFO FOR TABLE ro` shows
/// the table) and raises `An error occurred: …` only when the permission or
/// the assertion is actually evaluated.
#[test]
fn throw_in_a_predicate_is_not_a_syntax_error() {
    silent(
        "DEFINE TABLE ro SCHEMAFULL PERMISSIONS FOR select WHERE $auth != NONE, \
         FOR create, update, delete WHERE THROW \"Read-only customer\";",
    );
    silent("DEFINE FIELD f ON ro TYPE string ASSERT $value != NONE OR THROW \"y\";");
    // A thrown value is still a value: the condition contract (2005) reads a
    // diverging predicate as satisfying it, and the operand is still analyzed.
    silent("SELECT name FROM person WHERE THROW 'a';");
}

/// A bracket segment in a SET *target* was an `S0001`, on a statement 3.2.3
/// writes without complaint. `SET meta.score = 5` parsed; `SET meta['score']
/// = 5` did not.
///
/// Parsing it is only half the fix: every write check keys on a path of plain
/// field segments, so the statement would have gone from a wrong syntax error
/// to no diagnostic at all. A string-literal subscript is a field step (3.2.3
/// stores `meta: { score: 5 }` for `SET meta['score'] = 5`) and every other
/// subscript addresses an element, so the value is held to the element kind.
#[test]
fn a_bracket_segment_in_a_set_target_parses_and_still_checks_the_write() {
    silent("UPDATE person:1 SET tags[0] = 'ok';");
    silent("UPDATE person:1 SET meta['score'] = 5;");
    silent("UPDATE person:1 SET tags[$] = 'z';");
    silent("UPDATE person:1 SET tags[WHERE $this = 'a'] = 'z';");

    // The engine answers this one ``Couldn't coerce value for field `tags`:
    // Expected `none | array<string>` but found `[5]` ``; 2001 names the
    // element kind the write actually violated.
    let finding = only("UPDATE person:1 SET tags[0] = 5;", "E2001");
    assert_eq!(
        finding.message(),
        "`tags[…]` is declared `string`, but this value is `5`"
    );
    // A string subscript is a field step, so it reads as the field it is.
    let finding = only("UPDATE person:1 SET meta['score'] = 'x';", "E2001");
    assert_eq!(
        finding.message(),
        "`meta.score` is declared `int`, but this value is `'x'`"
    );
    // And the field still has to exist.
    let finding = only("UPDATE person:1 SET nope[0] = 1;", "E1002");
    assert_eq!(finding.message(), "`person` has no field `nope`");
}

/// `PATCH` took an array *literal* in the grammar, so a payload the engine
/// applies (`PATCH $ops`) and the common slip (one operation written without
/// its list) were both `S0001` — and an `S0001` suppresses every other
/// finding in the file.
///
/// 3.2.3 parses whatever follows `PATCH` and judges it when it runs: "The
/// JSON Patch contains invalid operations. Failed to parse JSON patch
/// structure: Patch operations should be an array of objects". That is 2033's
/// contract, and it could never fire while the parser rejected the statement
/// first.
#[test]
fn a_patch_payload_is_any_expression_and_2033_judges_it() {
    silent("UPDATE person:1 PATCH [{ op: 'replace', path: '/name', value: 'x' }];");
    // How a payload is normally passed — and it applies on 3.2.3, so a kind
    // that could still be a list at run time stays silent.
    silent("UPDATE person:1 PATCH $ops;");
    silent(
        "LET $ops = [{ op: 'replace', path: '/name', value: 'x' }]; UPDATE person:1 PATCH $ops;",
    );

    let finding = only(
        "UPDATE person:1 PATCH { op: 'replace', path: '/name', value: 'x' };",
        "E2033",
    );
    assert_eq!(
        finding.message(),
        "PATCH takes an array of operations, not a single object"
    );
    let finding = only("UPDATE person:1 PATCH 'x';", "E2033");
    assert_eq!(
        finding.message(),
        "PATCH takes an array of operations, but this is a `string`"
    );
}

/// `LIMIT`, `START` and `TIMEOUT` took a literal or a param in the grammar,
/// so the one spelling each contract exists to catch was a syntax error
/// instead: 3.2.3 *parses* `LIMIT '5'` and `TIMEOUT 5` and fails when it runs
/// them — "LIMIT/START must be an integer, got String(\"5\")" and "Invalid
/// timeout value". 2018 and 2019 already owned both contracts and already
/// reported them when the bad value arrived through a `LET`; only the literal
/// path degraded to `S0001`.
#[test]
fn a_literal_limit_or_timeout_reaches_2018_and_2019() {
    assert_eq!(
        only("SELECT name FROM person LIMIT '5';", "E2018").message(),
        "LIMIT needs an integer, but this is a `string`"
    );
    assert_eq!(
        only("SELECT name FROM person START '1';", "E2018").message(),
        "START needs an integer, but this is a `string`"
    );
    assert_eq!(
        only("SELECT name FROM person TIMEOUT 5;", "E2019").message(),
        "TIMEOUT needs a duration, but this is a `int`"
    );
    // The expression spellings the engine runs stay clean. (3.2.3 returns
    // rows for both `LIMIT 1 + 1` and `TIMEOUT 1s + 1s`.)
    for query in [
        "SELECT name FROM person LIMIT 1 + 1;",
        "SELECT name FROM person TIMEOUT 1s;",
        "SELECT name FROM person LIMIT $n;",
    ] {
        let found = findings(query);
        support::assert_no_syntax_findings(&found);
        assert!(
            !found
                .iter()
                .any(|finding| matches!(finding.code().number(), 2018 | 2019)),
            "`{query}` should be clean; got {:?}",
            found
                .iter()
                .map(|finding| finding.code().to_string())
                .collect::<Vec<_>>()
        );
    }
}
