//! The whole loop in one test: analyzer, engine, relation.
//!
//! The unit tests pin each piece against hand-written values. This pins the
//! join — that the kinds `analyze_workspace` produces and the values the
//! embedded engine returns line up statement for statement, and that the
//! missing-key adjustment is load-bearing on real engine output rather than
//! only on a constructed `Value`.

use surrealdb_types::Value;
use type_oracle::analyzer::analyze;
use type_oracle::engine::Engine;
use type_oracle::relation::{classify, Verdict};

const SCHEMA: &str = "\
DEFINE TABLE person SCHEMAFULL;
DEFINE FIELD name ON person TYPE string;
DEFINE FIELD nickname ON person TYPE option<string>;
";

const QUERY: &str = "\
CREATE person:ada SET name = 'Ada';
SELECT * FROM person;
RETURN [1, 2, 3][1..];
";

async fn observe(sources: &[&str]) -> Vec<Result<Value, String>> {
    let engine = Engine::fresh().await.expect("an in-memory engine");
    engine
        .select(Some("test"), Some("test"))
        .await
        .expect("a namespace and database");
    let (last, seeds) = sources.split_last().expect("at least one source");
    for seed in seeds {
        engine.seed(seed).await.expect("the schema runs");
    }
    engine.run(last).await.expect("the query runs")
}

#[tokio::test]
async fn a_stored_row_inhabits_its_inferred_kind_although_is_kind_says_otherwise() {
    let analyzed = analyze(&[
        ("schema.surql".to_string(), SCHEMA.to_string()),
        ("query.surql".to_string(), QUERY.to_string()),
    ]);
    let kinds = &analyzed["query.surql"].kinds;
    assert!(
        analyzed["query.surql"].errors.is_empty(),
        "the fixture must be valid SurrealQL: {:?}",
        analyzed["query.surql"].errors
    );

    let observed = observe(&[SCHEMA, QUERY]).await;
    assert_eq!(
        observed.len(),
        kinds.len(),
        "the analyzer and the engine must agree on the statement count"
    );

    // `SELECT * FROM person` — the stored row has no `nickname` key at all,
    // because SurrealDB does not store a NONE field.
    let kind = kinds[1].as_ref().expect("a response kind for the SELECT");
    let value = observed[1].as_ref().expect("the SELECT runs");
    assert!(
        !value.is_kind(kind),
        "the engine's own relation demands an exact key set — if this ever \
         starts passing, the missing-key adjustment can be deleted"
    );
    assert!(
        !matches!(classify(value, kind), Verdict::Mismatch(_)),
        "the row does inhabit the kind once a missing key is read as NONE"
    );
}

#[tokio::test]
async fn a_real_analyzer_bug_is_reported_as_a_mismatch() {
    let analyzed = analyze(&[
        ("schema.surql".to_string(), SCHEMA.to_string()),
        ("query.surql".to_string(), QUERY.to_string()),
    ]);
    let observed = observe(&[SCHEMA, QUERY]).await;

    // `[1, 2, 3][1..]` is a range slice; the analyzer reads it as an index and
    // says `int`, the engine returns `[2, 3]`. Baselined as a BUG.
    let kind = analyzed["query.surql"].kinds[2]
        .as_ref()
        .expect("a response kind for the slice");
    let value = observed[2].as_ref().expect("the slice runs");
    let Verdict::Mismatch(report) = classify(value, kind) else {
        panic!("an array does not inhabit `int` — this is the bug the oracle exists to catch");
    };
    assert_eq!(
        report.detail(),
        "<response>: observed array<int> does not inhabit int"
    );
}
