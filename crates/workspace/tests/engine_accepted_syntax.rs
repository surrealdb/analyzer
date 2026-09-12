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
