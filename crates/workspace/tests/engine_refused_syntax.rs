//! The shapes the grammar parses *because* the engine refuses them.
//!
//! Normally the grammar is the strict one and a bad query never reaches the
//! analyzer. For a handful of forms that is the wrong trade: SurrealDB's
//! parser gives up on the whole source with a message that points at a token,
//! and an editor showing "Unexpected token `;`, expected SINCE" over a
//! collapsed file helps nobody. So the grammar accepts the shape and the
//! analyzer names the contract at the span that is actually wrong.
//!
//! That is only correct while the diagnostic really fires — a form we parse
//! and say nothing about is a query we would pass as valid and SurrealDB
//! would reject. Each case below is checked against a live SurrealDB 3.2.3,
//! and its engine error is quoted in the message or help so the user sees
//! what they will actually be told.
//!
//! These also make our grammar a strict superset of upstream
//! `surrealql-tree-sitter`, whose corpus pins the KILL and SHOW cases as
//! parseable.

mod support;

use surrealql_analyzer_diagnostics::Finding;
use surrealql_analyzer_syntax::source::SourceId;
use surrealql_analyzer_workspace::{analyze_workspace, Workspace};

const SCHEMA: &str = "\
DEFINE TABLE person SCHEMAFULL CHANGEFEED 1d;
DEFINE FIELD name ON person TYPE string;
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

/// No finding may be a syntax error: the point of loosening the grammar is
/// that these parse.
fn assert_parses(query: &str) {
    for finding in findings(query) {
        assert!(
            !finding.code().to_string().starts_with("E0"),
            "`{query}` did not parse: {} {}",
            finding.code(),
            finding.message()
        );
    }
}

/// 3.2.3: `KILL "some-uuid-here"` is
/// "Unexpected token `a strand`, expected a UUID or a parameter".
#[test]
fn kill_with_a_plain_string_parses_and_raises_2020() {
    let query = "KILL \"some-uuid-here\";";
    assert_parses(query);
    let finding = only(query, "E2020");
    assert!(
        finding.message().contains("live-query uuid"),
        "unhelpful message: {}",
        finding.message()
    );
    let help: String = finding
        .help()
        .iter()
        .map(|help| help.message.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        help.contains("expected a UUID or a parameter"),
        "help should quote the engine's own error; got {help:?}"
    );
}

/// Uuid *shaped* is still a strand to the engine — 3.2.3 rejects
/// `KILL "018e0f3a-1234-7abc-8def-0123456789ab"` the same way.
#[test]
fn kill_with_a_uuid_shaped_string_still_raises_2020() {
    let query = "KILL \"018e0f3a-1234-7abc-8def-0123456789ab\";";
    assert_parses(query);
    only(query, "E2020");
}

/// The `u'…'` literal and a parameter are what the engine takes, so neither
/// may be flagged — the near-miss half of the contract.
#[test]
fn kill_with_a_uuid_literal_or_param_is_silent() {
    for query in [
        "KILL u'018e0f3a-1234-7abc-8def-0123456789ab';",
        "LET $id = u'018e0f3a-1234-7abc-8def-0123456789ab'; KILL $id;",
    ] {
        assert_parses(query);
        let codes: Vec<String> = findings(query)
            .iter()
            .map(|finding| finding.code().to_string())
            .collect();
        assert!(
            !codes.iter().any(|code| code == "E2020"),
            "`{query}` is valid KILL but raised 2020; got {codes:?}"
        );
    }
}

/// 3.2.3: `SHOW CHANGES FOR TABLE person;` is
/// "Unexpected token `;`, expected SINCE".
#[test]
fn show_changes_without_since_parses_and_raises_2021() {
    let query = "SHOW CHANGES FOR TABLE person;";
    assert_parses(query);
    let finding = only(query, "E2021");
    assert!(
        finding.message().contains("SINCE"),
        "unhelpful message: {}",
        finding.message()
    );
    let help: String = finding
        .help()
        .iter()
        .map(|help| help.message.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        help.contains("expected SINCE"),
        "help should quote the engine's own error; got {help:?}"
    );
}

/// `LIMIT` does not stand in for `SINCE`: 3.2.3 answers
/// `SHOW CHANGES FOR TABLE person LIMIT 10` with
/// "Unexpected token `LIMIT`, expected SINCE".
#[test]
fn show_changes_with_limit_but_no_since_parses_and_raises_2021() {
    let query = "SHOW CHANGES FOR TABLE person LIMIT 10;";
    assert_parses(query);
    only(query, "E2021");
}

/// The spelled-out form stays silent, so 2021 reports the missing clause and
/// not the statement.
#[test]
fn show_changes_with_since_is_silent() {
    for query in [
        "SHOW CHANGES FOR TABLE person SINCE 0;",
        "SHOW CHANGES FOR TABLE person SINCE 1 LIMIT 10;",
        "SHOW CHANGES FOR TABLE person SINCE '2024-01-01T00:00:00Z';",
    ] {
        assert_parses(query);
        let codes: Vec<String> = findings(query)
            .iter()
            .map(|finding| finding.code().to_string())
            .collect();
        assert!(
            !codes.iter().any(|code| code == "E2021"),
            "`{query}` names SINCE but raised 2021; got {codes:?}"
        );
    }
}

/// 3.2.3 takes `INSERT RELATION IGNORE`, in that order, and refuses the
/// reverse with a token error pointing at whatever follows the pair, which
/// says nothing about the two words before it. The grammar takes both orders
/// so 4030 can name the order instead.
///
/// `INTO` is optional, so the engine's quoted token varies —
/// ``Unexpected token `INTO`, expected Eof`` with it, ``Unexpected token
/// `{`, expected Eof`` without — and both shapes must raise 4030 with help
/// that is true of each.
#[test]
fn insert_with_the_modifiers_reversed_parses_and_raises_4030() {
    for query in [
        "INSERT IGNORE RELATION INTO person { name: 'Ada' };",
        "INSERT IGNORE RELATION { name: 'Ada' };",
    ] {
        assert_parses(query);
        let finding = only(query, "E4030");
        assert!(
            finding.message().contains("`RELATION` before `IGNORE`"),
            "unhelpful message: {}",
            finding.message()
        );
        let help: String = finding
            .help()
            .iter()
            .map(|help| help.message.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            help.contains("expected Eof"),
            "help should quote the engine's own error; got {help:?}"
        );
        assert!(
            !help.contains("`INTO`"),
            "help must not quote a token this statement may not contain; got {help:?}"
        );
    }
}

/// The order the engine takes, and each modifier on its own, stay silent —
/// so 4030 reports the order and not the statement.
#[test]
fn insert_in_the_order_the_engine_takes_is_silent() {
    for query in [
        "INSERT RELATION IGNORE INTO person { name: 'Ada' };",
        "INSERT RELATION INTO person { name: 'Ada' };",
        "INSERT IGNORE INTO person { name: 'Ada' };",
        "INSERT INTO person { name: 'Ada' };",
    ] {
        assert_parses(query);
        let codes: Vec<String> = findings(query)
            .iter()
            .map(|finding| finding.code().to_string())
            .collect();
        assert!(
            !codes.iter().any(|code| code == "E4030"),
            "`{query}` is a legal modifier order but raised 4030; got {codes:?}"
        );
    }
}

/// A live query is a much narrower statement than `SELECT`, and every clause
/// past `WHERE`/`FETCH` is one 3.2.3 refuses while *parsing* — verified live:
/// `LIVE SELECT * FROM ticket ORDER BY title` is ``Unexpected token `ORDER`,
/// expected Eof``, and so are `GROUP`, `LIMIT`, `START`, `SPLIT`, `OMIT`,
/// `TIMEOUT`, `PARALLEL`, `EXPLAIN` and `FROM ONLY`. The grammar takes them
/// so 4009 can name each one instead of the statement collapsing into a
/// token error.
#[test]
fn a_live_select_with_a_set_shaping_clause_parses_and_raises_4009() {
    for (query, fragment) in [
        ("LIVE SELECT * FROM person ORDER BY name;", "ORDER BY"),
        ("LIVE SELECT * FROM person GROUP BY name;", "GROUP BY"),
        ("LIVE SELECT * FROM person LIMIT 1;", "LIMIT"),
        ("LIVE SELECT * FROM person START 1;", "START"),
        ("LIVE SELECT * FROM person SPLIT name;", "SPLIT"),
        ("LIVE SELECT * OMIT name FROM person;", "OMIT"),
        ("LIVE SELECT * FROM person TIMEOUT 1s;", "TIMEOUT"),
        ("LIVE SELECT * FROM person PARALLEL;", "PARALLEL"),
        ("LIVE SELECT * FROM person EXPLAIN;", "EXPLAIN"),
        ("LIVE SELECT * FROM ONLY person;", "ONLY"),
    ] {
        assert_parses(query);
        let finding = only(query, "E4009");
        assert!(
            finding
                .message()
                .to_lowercase()
                .contains(&fragment.to_lowercase()),
            "`{query}` should name {fragment}: {}",
            finding.message()
        );
    }
}

/// A live query subscribes to one table: 3.2.3 stops at the comma —
/// `LIVE SELECT * FROM ticket, person` is ``Unexpected token `,`, expected
/// Eof``.
#[test]
fn a_live_select_over_two_tables_parses_and_raises_4009() {
    let query = "LIVE SELECT * FROM person, person;";
    assert_parses(query);
    only(query, "E4009");
}

/// The clauses a live query really does take stay silent, so 4009 reports
/// the unsupported clause and not the statement.
#[test]
fn a_live_select_with_only_where_and_fetch_is_silent() {
    for query in [
        "LIVE SELECT * FROM person;",
        "LIVE SELECT * FROM person WHERE name = 'x';",
        "LIVE SELECT * FROM person FETCH name;",
        "LIVE SELECT DIFF FROM person;",
    ] {
        assert_parses(query);
        let codes: Vec<String> = findings(query)
            .iter()
            .map(|finding| finding.code().to_string())
            .collect();
        assert!(
            !codes.iter().any(|code| code == "E4009"),
            "`{query}` is a legal live query but raised 4009; got {codes:?}"
        );
    }
}

/// A `COUNT` index takes no `FIELDS` — 3.2.3: `DEFINE INDEX icnt ON person
/// FIELDS name COUNT` is "Cannot create a count index with fields". The
/// grammar takes the combination (it is an independent repeated clause, not
/// a context-free restriction) so 1033 can name the mistake.
#[test]
fn a_count_index_naming_fields_parses_and_raises_1033() {
    let query = "DEFINE INDEX icnt ON person FIELDS name COUNT;";
    assert_parses(query);
    let finding = only(query, "E1033");
    assert!(finding.message().contains("COUNT"), "{}", finding.message());
    let help: String = finding
        .help()
        .iter()
        .map(|help| help.message.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        help.contains("Cannot create a count index with fields"),
        "help should quote the engine's own error; got {help:?}"
    );
}

/// A bare `COUNT` index, and a plain `FIELDS` index with no `COUNT`, are
/// exactly what each is for — silent.
#[test]
fn a_count_index_without_fields_is_silent() {
    for query in [
        "DEFINE INDEX icnt ON person COUNT;",
        "DEFINE INDEX icnt ON person COUNT WHERE name != '';",
        "DEFINE INDEX ifields ON person FIELDS name;",
    ] {
        assert_parses(query);
        let codes: Vec<String> = findings(query)
            .iter()
            .map(|finding| finding.code().to_string())
            .collect();
        assert!(
            !codes.iter().any(|code| code == "E1033"),
            "`{query}` is a legal index but raised 1033; got {codes:?}"
        );
    }
}
