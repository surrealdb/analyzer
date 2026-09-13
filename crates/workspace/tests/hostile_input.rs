//! Input that is small to write and expensive to analyze.
//!
//! Every case here was a crash or a hang: a four-kilobyte file of nested
//! parentheses aborted the process with `stack overflow` and no diagnostic a
//! host could render, a 110-byte nested object took forty-six seconds, and a
//! two-hundred-term `WHERE` took ten. None of it is exotic — generated SQL
//! writes long operator chains, and a malformed template writes deep nesting
//! — and all of it arrives through the same public entry point as valid work.
//!
//! The timing assertions are deliberately loose (seconds against a target of
//! milliseconds): they are there to catch a return of exponential or cubic
//! growth on a loaded CI box, not to measure anything.

use std::time::{Duration, Instant};

use surrealql_analyzer_diagnostics::Finding;
use surrealql_analyzer_workspace::{analyze_workspace, Workspace};

/// Runs `body` on a thread with the stack a consumer's **main** thread has.
///
/// The nesting budget in `surrealql_analyzer_syntax::lower` is sized against
/// that 8 MiB, and the test harness gives each test thread 2 MiB. Without
/// this the gate would measure the harness rather than the fix.
fn on_a_main_sized_stack<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(body)
        .expect("spawn")
        .join()
        .expect("the analyzer must not abort the process")
}

/// Analyzes one source and returns its findings — the whole public path.
fn findings(text: String) -> Vec<Finding> {
    let mut workspace = Workspace::default();
    workspace.add_virtual_source("hostile".into(), text);
    analyze_workspace(&workspace).diagnostics
}

/// Analyzes `text` on a main-sized stack and reports how long it took.
fn timed(text: String) -> (Vec<Finding>, Duration) {
    on_a_main_sized_stack(move || {
        let start = Instant::now();
        let found = findings(text);
        let elapsed = start.elapsed();
        (found, elapsed)
    })
}

fn cutoffs(found: &[Finding]) -> Vec<&Finding> {
    found
        .iter()
        .filter(|finding| finding.code().number() == 6003)
        .collect()
}

/// Every recursive syntactic form, five thousand deep. Each of these aborted
/// the process (SIGABRT, exit 134, no message) before the lowering budget
/// existed.
#[test]
fn nesting_five_thousand_deep_reports_a_cut_off_instead_of_aborting() {
    let cases: Vec<(&str, String)> = vec![
        (
            "parentheses",
            format!("RETURN {}1{};", "(".repeat(5000), ")".repeat(5000)),
        ),
        (
            "arrays",
            format!("RETURN {}1{};", "[".repeat(5000), "]".repeat(5000)),
        ),
        (
            "objects",
            format!("RETURN {}1{};", "{a:".repeat(5000), "}".repeat(5000)),
        ),
        (
            "blocks",
            format!("{}1{};", "IF true { ".repeat(5000), " }".repeat(5000)),
        ),
        (
            "subqueries",
            format!(
                "RETURN {}1{};",
                "(SELECT * FROM (".repeat(5000),
                "))".repeat(5000)
            ),
        ),
        (
            "operator chain",
            format!("RETURN {}1;", "1 + ".repeat(5000)),
        ),
    ];

    for (label, text) in cases {
        let found = on_a_main_sized_stack(move || findings(text));
        let cut_off = cutoffs(&found);
        assert_eq!(
            cut_off.len(),
            1,
            "{label}: one cut-off, not {}: {found:#?}",
            cut_off.len()
        );
        assert!(
            cut_off[0].message().contains("nests deeper"),
            "{label}: {}",
            cut_off[0].message()
        );
    }
}

/// The budget is a cut-off, not a cliff: nesting a consumer might plausibly
/// write is analyzed, and raises nothing.
#[test]
fn ordinary_nesting_is_analyzed_without_a_cut_off() {
    let text = format!("RETURN {}1{};", "(".repeat(32), ")".repeat(32));
    let found = findings(text);
    assert!(cutoffs(&found).is_empty(), "{found:#?}");
}

/// Nested object literals were exponential: the type of each property was
/// inferred twice, so depth 20 took 1.5 s, depth 25 took 46 s and depth 30
/// never finished. Depth 40 is 2^15 times depth 25's work on the old path.
#[test]
fn nested_object_literals_are_linear() {
    let text = format!("RETURN {}1{};", "{a:".repeat(40), "}".repeat(40));
    let (found, elapsed) = timed(text);
    assert!(cutoffs(&found).is_empty(), "{found:#?}");
    assert!(
        elapsed < Duration::from_secs(1),
        "40 nested objects took {elapsed:?}"
    );
}

/// A long `AND` chain is ordinary generated SQL (an expanded `IN` list, an
/// ORM filter). 200 conjuncts took 9.7 s and 300 timed out, because each link
/// re-derived — and re-inferred — the whole chain to its left.
#[test]
fn a_five_hundred_term_and_chain_is_analyzed_promptly() {
    let conjuncts: Vec<String> = (0..500).map(|i| format!("age > {i}")).collect();
    let text = format!(
        "DEFINE TABLE person SCHEMAFULL;\n\
         DEFINE FIELD age ON person TYPE int;\n\
         SELECT * FROM person WHERE {};",
        conjuncts.join(" AND ")
    );
    let (found, elapsed) = timed(text);
    assert!(cutoffs(&found).is_empty(), "{found:#?}");
    assert!(
        elapsed < Duration::from_secs(1),
        "500 conjuncts took {elapsed:?}"
    );
}

/// A chain of 500 terms is a thousand recursive frames across the inference
/// and checking walks, which is why this — like every long-chain case — runs
/// on a main-sized stack rather than the harness's 2 MiB.
///
/// `OR` runs the same narrowing machinery.
#[test]
fn a_five_hundred_term_or_chain_is_analyzed_promptly() {
    let disjuncts: Vec<String> = (0..500).map(|i| format!("age > {i}")).collect();
    let text = format!(
        "DEFINE TABLE person SCHEMAFULL;\n\
         DEFINE FIELD age ON person TYPE int;\n\
         SELECT * FROM person WHERE {};",
        disjuncts.join(" OR ")
    );
    let (_, elapsed) = timed(text);
    assert!(
        elapsed < Duration::from_secs(1),
        "500 disjuncts took {elapsed:?}"
    );
}

/// A statement the parser could not read has no structure a semantic check
/// can hold it to — it used to raise `S0001` for the text the parser choked
/// on *and* a semantic finding about the fragment that survived recovery.
///
/// The example is a bare `GROUP`: `LIVE SELECT … GROUP ALL` was this test's
/// input until the grammar learned every clause a live query takes, which is
/// what 4009 needs in order to report them.
#[test]
fn a_statement_that_failed_to_parse_gets_no_semantic_findings() {
    let text = "DEFINE TABLE person SCHEMAFULL;\n\
                DEFINE FIELD name ON person TYPE string;\n\
                SELECT count() FROM person GROUP;"
        .to_string();
    let found = findings(text);
    let codes: Vec<String> = found.iter().map(|f| f.code().to_string()).collect();
    assert_eq!(codes, ["S0001"], "{found:#?}");
}

/// The well-formed statements around a broken one keep their findings — the
/// suppression is one statement wide, not one file wide.
#[test]
fn a_broken_statement_does_not_silence_its_neighbours() {
    let text = "DEFINE TABLE person SCHEMAFULL;\n\
                DEFINE FIELD name ON person TYPE string;\n\
                SELECT count() FROM person GROUP;\n\
                SELECT nosuchfield FROM person;"
        .to_string();
    let found = findings(text);
    let codes: Vec<String> = found.iter().map(|f| f.code().to_string()).collect();
    assert!(codes.iter().any(|code| code == "S0001"), "{found:#?}");
    assert!(codes.iter().any(|code| code == "E1002"), "{found:#?}");
    assert!(!codes.iter().any(|code| code == "W4023"), "{found:#?}");
}

/// One parse failure, one finding. tree-sitter nests `ERROR` nodes, and this
/// statement raised an `S0001` for the whole statement and a second for the
/// operand inside it.
#[test]
fn one_parse_failure_raises_one_syntax_finding() {
    let text = "RELATE 'user:1' -> wrote -> post:1;".to_string();
    let found = findings(text.clone());
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].code().to_string(), "S0001");
    // The innermost report is the one that names the offending text.
    let range = found[0].span().range();
    assert_eq!(
        &text[range.start() as usize..range.end() as usize],
        "'user:1' ->"
    );
}
