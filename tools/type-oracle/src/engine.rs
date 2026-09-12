//! The engine side of the oracle: SurrealDB itself, embedded and in-process.
//!
//! `surreal sql --json` cannot be used for this. Six of the twelve leaf types
//! do not survive the JSON encoding — `1dec` and `d'…'` and `1h` and `r"t:1"`
//! all come back as strings, `NONE` and `NULL` collapse into `null`, bytes
//! become an int array — including the two distinctions the analyzer works
//! hardest to keep apart. The typed SDK `Value` preserves all of them, and
//! `Response::take::<Value>(i)` is a dedicated impl that hands back the raw
//! per-statement value with no `Option`/`Vec` unwrapping in the way.

use std::time::Duration;

use surrealdb::engine::local::{Db, Mem};
use surrealdb::opt::capabilities::Capabilities;
use surrealdb::opt::Config;
use surrealdb::Surreal;
use surrealdb_types::Value;

/// How long one file may take before it is abandoned. The language-test corpus
/// contains deliberately expensive queries; a hung file must not hang the run.
const FILE_TIMEOUT: Duration = Duration::from_secs(30);

/// An in-memory SurrealDB. Cheap enough to build one per test file, which is
/// what keeps every file's observations independent of every other file's.
pub struct Engine {
    db: Surreal<Db>,
}

impl Engine {
    /// A fresh, empty, in-memory database.
    ///
    /// Capabilities are the SDK defaults — every non-scripting function, no
    /// network access — plus all experimental features, because the language
    /// tests exercise them. Network stays denied on purpose: a test that
    /// reached out over HTTP would make the harness non-hermetic.
    pub async fn fresh() -> Result<Self, String> {
        let capabilities = Capabilities::default().with_all_experimental_features_allowed();
        let db = Surreal::new::<Mem>(Config::default().capabilities(capabilities))
            .await
            .map_err(|error| format!("could not start the embedded engine: {error}"))?;
        Ok(Self { db })
    }

    /// Select the namespace and database. Either may be absent: a language test
    /// can declare `namespace = false` and expect its statements to fail.
    pub async fn select(
        &self,
        namespace: Option<&str>,
        database: Option<&str>,
    ) -> Result<(), String> {
        if let Some(namespace) = namespace {
            self.db
                .use_ns(namespace)
                .await
                .map_err(|error| format!("use ns {namespace}: {error}"))?;
        }
        if let Some(database) = database {
            self.db
                .use_db(database)
                .await
                .map_err(|error| format!("use db {database}: {error}"))?;
        }
        Ok(())
    }

    /// Run one source and hand back one result per statement, in order.
    ///
    /// An `Err` for the whole call is a parse failure — the engine rejected the
    /// source before any statement ran. An `Err` in the vector is one statement
    /// the engine refused, which is a normal thing for this corpus to contain.
    pub async fn run(&self, sql: &str) -> Result<Vec<Result<Value, String>>, String> {
        let query = self.db.query(sql);
        let response = tokio::time::timeout(FILE_TIMEOUT, query)
            .await
            .map_err(|_| format!("the engine took longer than {FILE_TIMEOUT:?}"))?;
        let mut response = response.map_err(|error| error.to_string())?;
        let count = response.num_statements();
        Ok((0..count)
            .map(|index| {
                response
                    .take::<Value>(index)
                    .map_err(|error| error.to_string())
            })
            .collect())
    }

    /// Run a source for its effects only — an import, or the corpus schema.
    ///
    /// Returns the error of the first statement the engine refused. A seed that
    /// half-applied is worse than one that failed outright: every later
    /// comparison would be against a schema nobody declared, and the mismatches
    /// would look like inference bugs.
    pub async fn seed(&self, sql: &str) -> Result<(), String> {
        let results = self.run(sql).await?;
        match results.into_iter().find_map(Result::err) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// The engine version, for `[test] version` requirements.
    pub async fn version(&self) -> Result<semver::Version, String> {
        self.db.version().await.map_err(|error| error.to_string())
    }
}
