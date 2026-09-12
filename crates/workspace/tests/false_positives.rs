//! Valid SurrealQL the analyzer used to report, each beside the nearest
//! spelling that is genuinely wrong.
//!
//! `contract_guards.rs` pins one fire/silent pair per Deny code against a
//! shared fixture schema. This suite is the other direction: a case is entered
//! here because a *specific* construct was reported on code SurrealDB 3.2.3
//! accepts, and each one carries its own schema so the construct can be
//! written the way real code writes it. The `rejected` half is what keeps a
//! fix from degenerating into "stop checking this": the same construct spelled
//! in a way the engine really does refuse must still raise the same code.
//!
//! Every `accepted` string below was run against SurrealDB 3.2.3 and returned
//! without error; the `engine` note on each case records what it answered.

mod support;

use surrealql_analyzer_syntax::source::SourceId;
use surrealql_analyzer_workspace::config::WorkspaceConfig;
use surrealql_analyzer_workspace::{analyze_workspace, Workspace, WorkspaceAnalysis};

use support::assert_no_syntax_findings;

/// One false positive: the code that fired, the schema it needs, the valid
/// statement that must now be silent, and the invalid neighbour that must not
/// have gone silent with it.
struct Case {
    /// The catalog number that used to fire on `accepted`.
    code: u16,
    /// What SurrealDB 3.2.3 answers for `accepted`, verbatim enough to audit.
    engine: &'static str,
    /// Schema statements the case needs, analyzed as a schema source.
    schema: &'static str,
    /// Engine-accepted SurrealQL. Must raise no finding of `code`.
    accepted: &'static str,
    /// The nearest spelling the engine rejects. Must still raise `code`.
    rejected: &'static str,
}

const CASES: &[Case] = &[
    // ---- unrefined `record` into a `record<t>` destination -----------------
    Case {
        code: 2001,
        engine: "`UPDATE note SET owner = $auth` on a record-access session \
                 writes the row; the engine coerces `record` into `record<u>` \
                 and checks the table itself",
        schema: "DEFINE TABLE u SCHEMAFULL;\n\
                 DEFINE FIELD email ON u TYPE string;\n\
                 DEFINE TABLE note SCHEMAFULL;\n\
                 DEFINE FIELD owner ON note TYPE record<u>;\n\
                 DEFINE TABLE other SCHEMAFULL;\n",
        accepted: "UPDATE note SET owner = $auth;",
        rejected: "UPDATE note SET owner = other:1;",
    },
    Case {
        code: 5002,
        engine: "`RETURN fn::f($auth)` answers the record id and `\"OK\"`",
        schema: "DEFINE TABLE u SCHEMAFULL;\n\
                 DEFINE TABLE other SCHEMAFULL;\n\
                 DEFINE FUNCTION fn::f($a: record<u>) { RETURN $a; };\n",
        accepted: "RETURN fn::f($auth);",
        rejected: "RETURN fn::f(other:1);",
    },
    Case {
        code: 5002,
        engine: "a field declared bare `TYPE record` reaches a `record<t>` \
                 parameter the same way `$auth` does",
        schema: "DEFINE TABLE u SCHEMAFULL;\n\
                 DEFINE TABLE grant SCHEMAFULL;\n\
                 DEFINE FIELD subject ON grant TYPE record;\n\
                 DEFINE FIELD note ON grant TYPE string;\n\
                 DEFINE FUNCTION fn::f($a: record<u>) { RETURN $a; };\n",
        accepted: "SELECT fn::f(subject) AS ok FROM grant;",
        rejected: "SELECT fn::f(note) AS ok FROM grant;",
    },
    Case {
        // The near-miss for the `none | record` rule: a *declared* optional
        // record really can be unset, and writing it into a non-optional
        // destination really does fail.
        code: 5002,
        engine: "`$auth` is the analyzer's own `none | record` placeholder; a \
                 declared `option<record<u>>` is not",
        schema: "DEFINE TABLE u SCHEMAFULL;\n\
                 DEFINE TABLE note SCHEMAFULL;\n\
                 DEFINE FIELD owner ON note TYPE option<record<u>>;\n\
                 DEFINE FUNCTION fn::f($a: record<u>) { RETURN $a; };\n",
        accepted: "RETURN fn::f($auth);",
        rejected: "SELECT fn::f(owner) AS ok FROM note;",
    },
    // ---- an empty array literal inhabits `set<t>` --------------------------
    Case {
        code: 2001,
        engine: "`DEFINE FIELD tags ON t TYPE set<string> DEFAULT []` is \
                 accepted, as `array<string> DEFAULT []` already was",
        schema: "DEFINE TABLE t SCHEMAFULL;\n",
        accepted: "DEFINE FIELD tags ON t TYPE set<string> DEFAULT [];",
        rejected: "DEFINE FIELD tags ON t TYPE set<string> DEFAULT [1, 2];",
    },
    // ---- arrays and sets always compare ------------------------------------
    Case {
        // The near-miss is collection-against-scalar: only collection *pairs*
        // became comparable, so comparing an array to a string still reports.
        code: 2004,
        engine: "`RETURN (SELECT VALUE id FROM t LIMIT 1) != []` is `true`; \
                 `[1,2] != []`, `[1,2] != [3]` and `[1] != ['a']` are all `true`",
        schema: "DEFINE TABLE t SCHEMAFULL;\n\
                 DEFINE FIELD n ON t TYPE int;\n",
        accepted: "RETURN (SELECT VALUE id FROM t LIMIT 1) != [];",
        rejected: "RETURN [1, 2] = 'x';",
    },
    Case {
        code: 7005,
        engine: "`RETURN [1] = ['a']` is `false` — a result, not an error",
        schema: "DEFINE TABLE t SCHEMAFULL;\n",
        accepted: "RETURN [1, 2] != [];",
        rejected: "RETURN [1, 2] = ['a'];",
    },
    // ---- 1002 is a SCHEMAFULL contract -------------------------------------
    Case {
        code: 1002,
        engine: "`CREATE loose:a SET name = 'x', other = 5` on a SCHEMALESS \
                 table keeps `other`; only a SCHEMAFULL table drops it",
        schema: "DEFINE TABLE loose SCHEMALESS;\n\
                 DEFINE FIELD name ON loose TYPE string;\n\
                 DEFINE TABLE strict SCHEMAFULL;\n\
                 DEFINE FIELD name ON strict TYPE string;\n",
        accepted: "SELECT other FROM loose;",
        rejected: "SELECT other FROM strict;",
    },
    Case {
        code: 1002,
        engine: "a view materialises its projection's aliases, so \
                 `DEFINE INDEX itotal ON stats FIELDS total` builds",
        schema: "DEFINE TABLE t SCHEMAFULL;\n\
                 DEFINE FIELD n ON t TYPE int;\n\
                 DEFINE TABLE stats AS SELECT count() AS total FROM t GROUP ALL;\n",
        accepted: "DEFINE INDEX itotal ON stats FIELDS total;",
        rejected: "DEFINE INDEX ibad ON t FIELDS nope;",
    },
];

/// The query's analysis, with its own source id so the schema's findings are
/// never counted as the query's.
struct Analyzed {
    output: WorkspaceAnalysis,
    query: SourceId,
}

impl Analyzed {
    fn codes(&self, number: u16) -> usize {
        self.output
            .diagnostics
            .iter()
            .filter(|finding| finding.span().source() == &self.query)
            .filter(|finding| finding.code().number() == number)
            .count()
    }

    fn findings(&self) -> Vec<(String, String)> {
        self.output
            .diagnostics
            .iter()
            .filter(|finding| finding.span().source() == &self.query)
            .map(|finding| (finding.code().to_string(), finding.message().to_string()))
            .collect()
    }
}

fn analyze(schema: &str, query: &str) -> Analyzed {
    let mut workspace = Workspace::new(WorkspaceConfig::default());
    workspace.add_virtual_source("schema".into(), schema.into());
    let query = workspace.add_virtual_source("query".into(), query.into());
    let output = analyze_workspace(&workspace);
    assert_no_syntax_findings(&output.diagnostics);
    Analyzed { output, query }
}

#[test]
fn no_case_reports_the_form_the_engine_accepts() {
    for case in CASES {
        let analyzed = analyze(case.schema, case.accepted);
        assert_eq!(
            analyzed.codes(case.code),
            0,
            "{} fired on `{}`, which the engine accepts ({}); got {:?}",
            case.code,
            case.accepted,
            case.engine,
            analyzed.findings()
        );
    }
}

#[test]
fn every_case_still_reports_its_genuinely_wrong_neighbour() {
    for case in CASES {
        let analyzed = analyze(case.schema, case.rejected);
        assert!(
            analyzed.codes(case.code) >= 1,
            "{} stopped firing on `{}`, which is still wrong; got {:?}",
            case.code,
            case.rejected,
            analyzed.findings()
        );
    }
}
