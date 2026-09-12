//! What the generation path can still be held to in Rust alone.
//!
//! This file used to be `golden.rs`, and its centrepiece was a byte-for-byte
//! comparison of the rendered module against a committed golden that lived at
//! `packages/client/test-d/gen/surrealql-analyzer.generated.ts`. That golden
//! was load-bearing in two directions at once: this test pinned the emitter's
//! output against it, and `pnpm -r run typecheck` compiled it as part of
//! `@surrealdb/analyzer-client`, against that package's source and the real
//! `surrealdb` types, with `test-d/gen/*.test-d.ts` asserting what the
//! resolved types were. The second half is what gave the first its meaning:
//! Rust cannot tell whether a string it produced is valid TypeScript, so the
//! byte comparison alone only ever proved that today's output equals
//! yesterday's.
//!
//! The client package has left this repository, and nothing here compiles
//! TypeScript any more. A relocated golden would keep the comparison and lose
//! the compiler — a snapshot of an emitter whose target audience is gone, held
//! against a file the redesign is going to replace wholesale. Kept, it would
//! read like coverage of the generated module; it would not be any. So it is
//! dropped, deliberately, rather than moved.
//!
//! What survives is everything Rust genuinely can check about this path, and
//! both invariants are about the *analysis* feeding the emitter rather than
//! about TypeScript:
//!
//! 1. the fixture workspace analyzes clean, so nothing downstream is inferred
//!    against a broken schema, and
//! 2. every embedded query the fixture declares reaches the registry — a query
//!    the extractor silently dropped is a query the generated module would
//!    never mention.
//!
//! The fixture (`tests/fixtures/typecheck/`: schema with option/record/array/
//! literal-union/object fields, an edge table, `fn::` functions, a host
//! `src/queries.ts`) is kept for the same reason `surrealql-analyzer-codegen`
//! itself is: it is the seed of the redesigned typegen, and the shape it
//! exercises is the shape that will have to be re-emitted.

use std::path::{Path, PathBuf};

use surrealql_analyzer_codegen::{render_registry, QueryEntry};
use surrealql_analyzer_diagnostics::Severity;
use surrealql_analyzer_workspace::config::WorkspaceConfig;
use surrealql_analyzer_workspace::{analyze_workspace, Workspace};

/// The fixture workspace: a `surrealql-analyzer.toml`, schema and query `.surql`
/// files, and a host `src/queries.ts` carrying the embedded queries.
fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/typecheck")
}

/// The fixture must be valid input. Every type this crate renders comes out
/// of the analysis of these sources, so a fixture with a schema error would
/// have the emitter exercised against types inferred from a broken schema —
/// and the other test here would then be pinning nonsense. The bar is
/// therefore absolute: no error finding anywhere.
#[test]
fn fixture_is_free_of_error_findings() {
    let root = fixture_root();
    let (workspace, _, config) = load(&root);
    let analysis = analyze_workspace(&workspace);
    let policy = config.policy();
    let errors: Vec<String> = analysis
        .diagnostics
        .iter()
        .filter(|finding| {
            policy.resolve_severity(finding.code(), finding.severity()) == Some(Severity::Error)
        })
        .map(|finding| {
            format!(
                "  {} {} in {}",
                surrealql_analyzer_diagnostics::render_code(finding.code(), finding.severity()),
                finding.message(),
                finding.span().source().as_str()
            )
        })
        .collect();
    assert!(
        errors.is_empty(),
        "the typecheck fixture must analyze clean, but it reported {} error finding(s):\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// Every embedded query the fixture declares reaches the registry, keyed by
/// the text the caller passes. This is the one end-to-end claim that survives
/// without a TypeScript compiler: extraction → analysis → entry → rendered
/// module, with a query that fell out anywhere along the way visible as a
/// missing key rather than as a module that merely came out shorter.
#[test]
fn every_embedded_query_reaches_the_registry() {
    let root = fixture_root();
    let (_, queries, _) = load(&root);
    let module = generate(&root);
    for (_, query, _) in &queries {
        // A hole is keyed by the name the analyzer bound it to (`$__host0`,
        // `$__host1`, …), which is what the client reconstructs at call time.
        let mut key = String::new();
        for (index, part) in query.parts().iter().enumerate() {
            if index > 0 {
                key.push_str(&format!(
                    "${}{}",
                    surrealql_analyzer_embed::HOST_PARAM_PREFIX,
                    index - 1
                ));
            }
            key.push_str(part);
        }
        assert!(
            module.contains(&format!("    \"{}\": {{", key.replace('"', "\\\""))),
            "embedded query is missing from the generated registry: {key}"
        );
    }
    assert!(
        queries.len() >= 15,
        "the fixture host file should carry the whole spread of query forms, found {}",
        queries.len()
    );
}

/// One embedded query: the virtual source it was registered under, the
/// extraction (for its template parts), and the host file it came from.
type Embedded = (
    surrealql_analyzer_syntax::source::SourceId,
    surrealql_analyzer_embed::EmbeddedQuery,
    String,
);

/// Runs the generation path over `root` and returns the rendered module:
/// `QueryEntry::from_analysis` over each embedded query's analysis, then
/// `render_registry`. Nothing compiles the result — see the module docs.
fn generate(root: &Path) -> String {
    let (workspace, queries, _) = load(root);
    let analysis = analyze_workspace(&workspace);
    let entries: Vec<QueryEntry> = queries
        .iter()
        .filter_map(|(source_id, query, _)| {
            let output = analysis.sources.get(source_id)?;
            Some(QueryEntry::from_analysis(query.parts(), output))
        })
        .collect();
    render_registry(&entries)
}

/// Loads the fixture the way the CLI loads a workspace: config from
/// `surrealql-analyzer.toml`, `.surql` files in sorted path order with schema
/// sources (those matching a `[sources] schema` glob) registered first, then
/// every host file's embedded queries as virtual sources.
///
/// Paths are registered relative to `root` so source ids do not depend on
/// where the repository is checked out, and the registration order is
/// deterministic — nothing in the output names a source id, but the order
/// decides which definition wins when two sources declare the same name.
fn load(root: &Path) -> (Workspace, Vec<Embedded>, WorkspaceConfig) {
    let config_text = std::fs::read_to_string(root.join("surrealql-analyzer.toml"))
        .expect("fixture surrealql-analyzer.toml");
    let config = WorkspaceConfig::from_toml_str(&config_text).expect("fixture config parses");
    let mut workspace = Workspace::new(config.clone());

    let files = walk(root);
    let (schema, queries): (Vec<&PathBuf>, Vec<&PathBuf>) = files
        .iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "surql")
        })
        .partition(|path| matches_any_glob(root, path, &config.sources.schema));
    for path in schema.into_iter().chain(queries) {
        let text = read(path);
        workspace.add_virtual_source(relative(root, path), text);
    }

    let mut embedded = Vec::new();
    for path in files.iter().filter(|path| is_host_source(path)) {
        let text = read(path);
        let host_id = relative(root, path);
        for (index, query) in surrealql_analyzer_embed::extract(&host_id, &text)
            .into_iter()
            .enumerate()
        {
            let source_id = workspace
                .add_virtual_source(format!("embedded://{host_id}#{index}"), query.text.clone());
            embedded.push((source_id, query, host_id.clone()));
        }
    }

    (workspace, embedded, config)
}

/// Every file under `root`, in sorted path order — the order the CLI's
/// directory walk yields after its own sort.
fn walk(root: &Path) -> Vec<PathBuf> {
    fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read fixture dir") {
            let path = entry.expect("fixture dir entry").path();
            if path.is_dir() {
                visit(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    visit(root, &mut files);
    files.sort();
    files
}

/// The CLI's host-file extension set (`is_host_source` in `crates/cli`).
fn is_host_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "svelte" | "vue" | "astro")
    )
}

fn matches_any_glob(root: &Path, path: &Path, globs: &[String]) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    globs
        .iter()
        .any(|glob| glob::Pattern::new(glob).is_ok_and(|pattern| pattern.matches_path(relative)))
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("fixture file under the fixture root")
        .to_string_lossy()
        .replace('\\', "/")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}
