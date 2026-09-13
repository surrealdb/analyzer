//! The verbs against real directories, through the public API a host uses.
//!
//! These were the standalone CLI's behavioural tests; they moved here when
//! the binary went away, so the contracts a host relies on — schema before
//! queries, embedded findings at their host file, no registry written on an
//! error — are pinned at the library boundary rather than behind `clap`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use surrealql_analyzer::generate::client_package_is_resolvable;
use surrealql_analyzer::workspace::config::WorkspaceConfig;
use surrealql_analyzer::{check, generate, GenerateError, Project, Styles};

fn temp_project_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("surrealql-analyzer-{name}-{unique}"));
    fs::create_dir_all(&root).expect("create temp project root");
    root
}

fn discover(root: &Path) -> Project {
    Project::discover(root).expect("config parses")
}

#[test]
fn a_host_builds_a_project_without_a_config_file() {
    // The SurrealKit path: the host knows its schema directory from its own
    // layout and hands the analyzer a config directly. No
    // `surrealql-analyzer.toml` is written or read.
    let root = temp_project_dir("host-config");
    fs::create_dir_all(root.join("database/schema")).expect("schema dir");
    fs::write(
        root.join("database/schema/user.surql"),
        "DEFINE TABLE user SCHEMAFULL;\nDEFINE FIELD name ON user TYPE string;",
    )
    .expect("write schema");
    fs::write(root.join("bad.surql"), "SELECT nope FROM user;").expect("write query");

    let mut config = WorkspaceConfig::default();
    config.sources.schema = vec!["database/schema/**/*.surql".into()];
    let project = Project::new(&root, config);

    let report = check(&project).expect("analysis runs");
    assert!(!report.passed());
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.iter().any(|c| c.starts_with("E1002")), "{codes:?}");
    assert!(!codes.iter().any(|c| c.starts_with("E1001")), "{codes:?}");
}

#[test]
fn schema_sources_are_analyzed_before_query_sources() {
    // A `schema/` file and a `queries/` file: the query must see the
    // schema even though "queries" sorts before "schema" by path. A
    // reference to a real table must NOT report `unknown table`, and a
    // bad field must report the precise `unknown field`.
    let root = temp_project_dir("schema-order");
    fs::create_dir_all(root.join("schema")).expect("schema dir");
    fs::create_dir_all(root.join("queries")).expect("queries dir");
    fs::write(
        root.join("surrealql-analyzer.toml"),
        "[sources]\nschema = [\"schema/**/*.surql\"]\nqueries = [\"queries/**/*.surql\"]\n",
    )
    .expect("write config");
    fs::write(
        root.join("schema/user.surql"),
        "DEFINE TABLE user SCHEMAFULL;\nDEFINE FIELD name ON user TYPE string;",
    )
    .expect("write schema");
    fs::write(root.join("queries/bad.surql"), "SELECT nope FROM user;").expect("write query");

    let report = check(&discover(&root)).expect("analysis runs");
    assert!(!report.passed(), "the unknown field should fail the check");
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.iter().any(|c| c.starts_with("E1002")),
        "expected unknown-field E1002, got {codes:?}"
    );
    assert!(
        !codes.iter().any(|c| c.starts_with("E1001")),
        "schema was not applied before the query — spurious unknown-table: {codes:?}"
    );
}

#[test]
fn check_loads_surrealql_files_through_workspace_analysis() {
    let root = temp_project_dir("valid-check");
    fs::create_dir_all(root.join("schema")).expect("create schema dir");
    fs::write(root.join("surrealql-analyzer.toml"), "").expect("write config");
    fs::write(root.join("schema/person.surql"), "DEFINE TABLE person;").expect("write schema");

    let report = check(&discover(&root)).expect("valid workspace should check");

    assert!(report.passed());
    assert_eq!(report.summary.sources_checked, 1);
    assert_eq!(report.summary.diagnostics, 0);
    assert_eq!(report.summary.errors, 0);
}

#[test]
fn check_renders_syntax_errors_against_the_source() {
    let root = temp_project_dir("invalid-check");
    fs::create_dir_all(root.join("queries")).expect("create queries dir");
    fs::write(root.join("surrealql-analyzer.toml"), "").expect("write config");
    fs::write(root.join("queries/bad.surql"), "SELECT * FROM ;").expect("write query");

    let report = check(&discover(&root)).expect("analysis runs");
    assert!(!report.passed(), "syntax diagnostics should fail check");

    // `FROM ;` is a missing table name: a MISSING node (S0002), not skipped input.
    let rendered = report.render(Styles::plain()).join("\n");
    assert!(rendered.contains("S0002"), "{rendered}");
    assert!(rendered.contains("bad.surql"), "{rendered}");
}

#[test]
fn discover_finds_config_from_a_parent_directory() {
    let root = temp_project_dir("parent-config-check");
    let child = root.join("nested").join("project");
    fs::create_dir_all(&child).expect("create child dir");
    fs::write(root.join("surrealql-analyzer.toml"), "").expect("write config");
    fs::write(root.join("person.surql"), "DEFINE TABLE person;").expect("write schema");

    let project = discover(&child);
    assert_eq!(project.root(), root.as_path());
    let report = check(&project).expect("valid parent workspace should check");
    assert_eq!(report.summary.sources_checked, 1);
}

#[test]
fn check_json_uses_stable_diagnostic_keys() {
    let root = temp_project_dir("json-check");
    fs::create_dir_all(root.join("queries")).expect("create queries dir");
    fs::write(root.join("surrealql-analyzer.toml"), "").expect("write config");
    fs::write(root.join("queries/bad.surql"), "SELECT * FROM ;").expect("write query");

    let report = check(&discover(&root)).expect("analysis runs");
    let json = report.to_json().expect("json renders");
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");

    assert_eq!(value["summary"]["sources_checked"], 1);
    assert_eq!(value["summary"]["diagnostics"], 1);
    assert_eq!(value["summary"]["errors"], 1);
    assert_eq!(value["diagnostics"][0]["code"], "S0002");
    assert_eq!(value["diagnostics"][0]["severity"], "error");
    // Project-root-relative, with no `file://` scheme and no absolute prefix:
    // two machines analyzing the same commit must produce the same document,
    // and a consumer must not have to know whether a finding landed in a
    // `.surql` file or a host file to parse the path.
    assert_eq!(value["diagnostics"][0]["source"], "queries/bad.surql");
    assert_eq!(
        value["diagnostics"][0]["message"],
        "missing SurrealQL syntax node `Ident`"
    );
    assert!(value["diagnostics"][0]["range"]["start"].is_number());
    assert!(value["diagnostics"][0]["range"]["end"].is_number());
}

#[test]
fn the_registry_path_defaults_to_the_root_and_honours_out() {
    let root = temp_project_dir("registry-path");
    let project = Project::new(&root, WorkspaceConfig::default());
    assert_eq!(
        project.registry_path(None),
        root.join("surrealql-analyzer.generated.ts")
    );
    let explicit = root.join("custom.ts");
    assert_eq!(project.registry_path(Some(&explicit)), explicit);
}

#[test]
fn warnings_as_errors_config_fails_the_check_on_a_warning_finding() {
    // A DROP table with a declared field yields only the 4022 warning
    // (SELECT from a DROP table) with no lint noise.
    let root = temp_project_dir("warn-as-error");
    fs::write(
        root.join("surrealql-analyzer.toml"),
        "[diagnostics]\nwarnings_as_errors = true\n",
    )
    .expect("write config");
    fs::write(
        root.join("schema.surql"),
        "DEFINE TABLE t DROP SCHEMAFULL;\nDEFINE FIELD x ON t TYPE int;\nSELECT * FROM t;",
    )
    .expect("write source");

    let report = check(&discover(&root)).expect("analysis runs");
    assert!(!report.passed(), "promoted warning should fail the check");
    assert_eq!(report.summary.errors, 1);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E4022" && diagnostic.severity == "error"));
}

#[test]
fn warning_only_source_passes_without_warnings_as_errors() {
    let root = temp_project_dir("warn-only-clean");
    fs::write(root.join("surrealql-analyzer.toml"), "").expect("write config");
    fs::write(
        root.join("schema.surql"),
        "DEFINE TABLE t DROP SCHEMAFULL;\nDEFINE FIELD x ON t TYPE int;\nSELECT * FROM t;",
    )
    .expect("write source");

    // The 4022 warning is reported but keeps the check clean: it counts
    // as a diagnostic, not an error — and a clean run is not a silent run,
    // so the warning is still listed.
    let report = check(&discover(&root)).expect("warning-only source should pass");
    assert!(report.passed());
    assert_eq!(report.summary.diagnostics, 1);
    assert_eq!(report.summary.errors, 0);
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].severity, "warning");
}

#[test]
fn error_finding_fails_the_check_without_any_policy() {
    let root = temp_project_dir("error-fails");
    fs::write(root.join("surrealql-analyzer.toml"), "").expect("write config");
    // An unknown table is an error-class finding (E1001).
    fs::write(root.join("query.surql"), "SELECT * FROM ghost;").expect("write source");

    let report = check(&discover(&root)).expect("analysis runs");
    assert!(!report.passed());
    assert!(report.summary.errors >= 1);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E1001" && diagnostic.severity == "error"));
}

#[test]
fn generate_is_blocked_by_an_embedded_query_error_and_names_the_host_file() {
    // A host file whose `db.query("...")` targets a missing table must fail
    // generation, name the host file, and leave no registry behind.
    let root = temp_project_dir("generate-bad-embed");
    fs::create_dir_all(root.join("schema")).expect("schema dir");
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(
        root.join("surrealql-analyzer.toml"),
        "[sources]\nschema = [\"schema/**/*.surql\"]\n",
    )
    .expect("write config");
    fs::write(
        root.join("schema/person.surql"),
        "DEFINE TABLE person SCHEMAFULL;\nDEFINE FIELD name ON person TYPE string;",
    )
    .expect("write schema");
    fs::write(
        root.join("src/app.ts"),
        "const [rows] = await db.query(\"SELECT nope FROM missing\");",
    )
    .expect("write host source");

    let out = root.join("surrealql-analyzer.generated.ts");
    let error = generate(&discover(&root), Some(&out))
        .expect_err("an error-severity embedded query must block generate");
    let GenerateError::Blocked(blocked) = error else {
        panic!("expected a blocked generate, got {error}");
    };
    assert_eq!(blocked.errors, 1);
    let message = format!("{}\n{blocked}", blocked.render(Styles::plain()).join("\n"));
    // The finding must map back to the host file at a real line:col (not the
    // degraded `file:start..end` offset form), with the query's table underlined.
    assert!(
        message.contains("app.ts:1:"),
        "findings must map to the host file at line:col: {message}"
    );
    assert!(
        message.contains("db.query(\"SELECT nope FROM missing\")"),
        "the rendered snippet must show the host source line: {message}"
    );
    assert!(
        message.contains('^'),
        "the rendered snippet must underline the offending span: {message}"
    );
    assert!(
        message.contains("registry not written"),
        "failure must explain the registry was withheld: {message}"
    );
    assert!(
        blocked
            .diagnostics
            .iter()
            .all(|d| d.source.contains("app.ts")),
        "the structured findings must carry host coordinates too: {:?}",
        blocked.diagnostics
    );
    assert!(
        !out.exists(),
        "a broken registry must never be written on an error finding"
    );
}

#[test]
fn generate_writes_registry_for_clean_embedded_queries() {
    // A clean `db.query("...")` resolves its result and params from the
    // schema and lands in the registry, keyed by the exact query text.
    let root = temp_project_dir("generate-clean-embed");
    fs::create_dir_all(root.join("schema")).expect("schema dir");
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(
        root.join("surrealql-analyzer.toml"),
        "[sources]\nschema = [\"schema/**/*.surql\"]\n",
    )
    .expect("write config");
    fs::write(
        root.join("schema/person.surql"),
        "DEFINE TABLE person SCHEMAFULL;\n\
         DEFINE FIELD name ON person TYPE string;\n\
         DEFINE FIELD team ON person TYPE record<team>;\n\
         DEFINE TABLE team SCHEMAFULL;\n\
         DEFINE FIELD name ON team TYPE string;",
    )
    .expect("write schema");
    fs::write(
        root.join("src/app.ts"),
        "const [rows] = await db.query(\"SELECT name FROM person WHERE team = $team\", { team });",
    )
    .expect("write host source");

    let out = root.join("surrealql-analyzer.generated.ts");
    let report =
        generate(&discover(&root), Some(&out)).expect("a clean embedded query should generate");
    assert_eq!(report.path, out);
    assert_eq!(report.queries, 1);
    let written = fs::read_to_string(&out).expect("registry file written");
    assert_eq!(
        written, report.module,
        "the report carries what was written"
    );
    assert!(
        written.contains("SELECT name FROM person WHERE team = $team"),
        "registry must key the embedded query by its exact text:\n{written}"
    );
    assert!(
        written.contains("params: { team: RecordId<\"team\"> }"),
        "the $team param must be typed from the schema record link:\n{written}"
    );
}

#[test]
fn generate_creates_the_directory_the_registry_is_written_into() {
    // `out = "src/lib/db.generated.ts"` is the documented shape, and
    // `src/lib/` does not exist in a project that has not made it yet. The
    // write must create the directory rather than fail with an ENOENT that
    // names the file instead of the missing parent.
    let root = temp_project_dir("generate-makes-out-dir");
    fs::create_dir_all(root.join("schema")).expect("schema dir");
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(
        root.join("surrealql-analyzer.toml"),
        "[sources]\nschema = [\"schema/**/*.surql\"]\n",
    )
    .expect("write config");
    fs::write(
        root.join("schema/person.surql"),
        "DEFINE TABLE person SCHEMAFULL;\nDEFINE FIELD name ON person TYPE string;",
    )
    .expect("write schema");
    fs::write(
        root.join("src/app.ts"),
        "const [rows] = await db.query(\"SELECT name FROM person\");",
    )
    .expect("write host source");

    let out = root.join("src/lib/db.generated.ts");
    assert!(
        !out.parent().expect("out has a parent").exists(),
        "the fixture must start without the output directory"
    );

    let report = generate(&discover(&root), Some(&out)).expect("generate creates the directory");
    assert_eq!(report.path, out);
    let written = fs::read_to_string(&out).expect("registry file written");
    assert!(
        written.contains("SELECT name FROM person"),
        "registry must carry the embedded query:\n{written}"
    );
}

#[test]
fn check_reports_errors_in_embedded_host_queries_at_the_host_file() {
    // `check` is the CI gate. It must see the queries the client actually
    // runs — a host file's embedded query — or CI passes green on a
    // workspace whose `generate` then fails.
    let root = temp_project_dir("check-embedded-error");
    fs::create_dir_all(root.join("schema")).expect("schema dir");
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(
        root.join("surrealql-analyzer.toml"),
        "[sources]\nschema = [\"schema/**/*.surql\"]\n",
    )
    .expect("write config");
    fs::write(root.join("schema/t.surql"), "DEFINE TABLE t SCHEMAFULL;").expect("write schema");
    fs::write(
        root.join("src/app.ts"),
        "const q = db.query(\"SELECT * FROM nonexistent_table;\");",
    )
    .expect("write host source");

    let report = check(&discover(&root)).expect("analysis runs");
    assert!(
        !report.passed(),
        "an embedded unknown table must fail the check"
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code.starts_with("E1001")),
        "expected unknown-table E1001, got {:?}",
        report
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );
    // The finding must be reported against the host file, not the internal
    // `virtual://…` source the query was analyzed under.
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.source.contains("app.ts") && !d.source.starts_with("virtual://")),
        "embedded findings must map to the host file: {:?}",
        report
            .diagnostics
            .iter()
            .map(|d| &d.source)
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_passes_a_valid_embedded_host_query() {
    // The mirror of the above: scanning host files must not invent findings
    // on a query that is perfectly valid against the schema.
    let root = temp_project_dir("check-embedded-clean");
    fs::create_dir_all(root.join("schema")).expect("schema dir");
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(
        root.join("surrealql-analyzer.toml"),
        "[sources]\nschema = [\"schema/**/*.surql\"]\n",
    )
    .expect("write config");
    fs::write(
        root.join("schema/t.surql"),
        "DEFINE TABLE t SCHEMAFULL;\nDEFINE FIELD price ON t TYPE int;",
    )
    .expect("write schema");
    fs::write(
        root.join("src/app.ts"),
        "const q = db.query(\"SELECT price FROM t;\");",
    )
    .expect("write host source");

    let report = check(&discover(&root)).expect("a valid embedded query must pass");
    assert!(report.passed());
    assert_eq!(report.summary.errors, 0);
}

/// Writes a resolvable `@surrealdb/analyzer-client` under `dir/node_modules`.
fn install_client(dir: &Path) {
    let package = dir.join("node_modules/@surrealdb/analyzer-client");
    fs::create_dir_all(&package).expect("create package dir");
    fs::write(
        package.join("package.json"),
        "{\"name\":\"@surrealdb/analyzer-client\"}",
    )
    .expect("write package.json");
}

/// The workspace `generate` needs to produce a module at all: a schema and
/// one clean embedded query.
fn generatable_project(name: &str) -> PathBuf {
    let root = temp_project_dir(name);
    fs::create_dir_all(root.join("schema")).expect("schema dir");
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(
        root.join("surrealql-analyzer.toml"),
        "[sources]\nschema = [\"schema/**/*.surql\"]\n",
    )
    .expect("write config");
    fs::write(
        root.join("schema/person.surql"),
        "DEFINE TABLE person SCHEMAFULL;\nDEFINE FIELD name ON person TYPE string;",
    )
    .expect("write schema");
    fs::write(
        root.join("src/app.ts"),
        "const [rows] = await db.query(\"SELECT name FROM person\");",
    )
    .expect("write host source");
    root
}

#[test]
fn generate_warns_when_the_augmented_package_is_not_installed() {
    // The worst failure a type generator has: the module augments
    // `@surrealdb/analyzer-client`, the package is absent, TypeScript reports
    // TS2664 *inside the generated file*, drops the augmentation, and every
    // query in the user's own code silently becomes `any` with no error on
    // it. Nothing in that chain points at the missing dependency, so
    // `generate` has to.
    let root = generatable_project("generate-missing-client");

    let report = generate(&discover(&root), None).expect("generate writes");
    assert!(
        report.missing_client,
        "an unresolvable augmentation target must be reported"
    );
    let warning = report
        .render_missing_client(Styles::plain())
        .expect("the flag has a rendering");

    assert!(warning.contains("`@surrealdb/analyzer-client` is not installed"));
    assert!(
        warning.contains("npm install @surrealdb/analyzer-client surrealdb"),
        "the warning must name the command that fixes it: {warning}"
    );
    assert!(
        warning.contains("TS2664"),
        "naming the error TypeScript reports is what makes it searchable: {warning}"
    );
    assert!(
        warning.contains("`any`"),
        "the consequence is the reason this warning exists: {warning}"
    );
}

#[test]
fn generate_is_silent_when_the_package_resolves() {
    let root = generatable_project("generate-client-present");
    install_client(&root);

    let report = generate(&discover(&root), None).expect("generate writes");
    assert!(!report.missing_client, "an installed package must not warn");
    assert!(report.render_missing_client(Styles::plain()).is_none());
}

#[test]
fn resolution_walks_up_from_the_output_directory_the_way_node_does() {
    // The module is resolved from the directory it is written to, not from
    // the workspace root — a hoisted install two directories up resolves,
    // and in a monorepo those are routinely different packages.
    let root = generatable_project("generate-client-hoisted");
    let nested = root.join("packages/app/src");
    fs::create_dir_all(&nested).expect("nested dirs");
    install_client(&root);

    assert!(client_package_is_resolvable(&nested), "hoisted install");
    assert!(
        !client_package_is_resolvable(Path::new("/")),
        "a tree with no node_modules must not resolve"
    );

    // An empty package directory is not an install: neither Node nor
    // TypeScript resolves one, so neither does this.
    let bare = temp_project_dir("generate-client-empty-dir");
    fs::create_dir_all(bare.join("node_modules/@surrealdb/analyzer-client"))
        .expect("empty package");
    assert!(!client_package_is_resolvable(&bare));
}

#[test]
fn a_file_that_is_not_utf8_is_reported_and_the_rest_of_the_run_continues() {
    // One stray latin-1 or binary file used to abort the whole run: exit 2,
    // "stream did not contain valid UTF-8", and not one diagnostic for the
    // files that were fine. The file is now one finding, at the byte that is
    // not UTF-8, and every other source is still analyzed.
    let root = temp_project_dir("not-utf8");
    fs::create_dir_all(root.join("schema")).expect("schema dir");
    fs::create_dir_all(root.join("queries")).expect("queries dir");
    fs::write(
        root.join("surrealql-analyzer.toml"),
        "[sources]\nschema = [\"schema/**/*.surql\"]\nqueries = [\"queries/**/*.surql\"]\n",
    )
    .expect("write config");
    fs::write(
        root.join("schema/user.surql"),
        "DEFINE TABLE user SCHEMAFULL;\nDEFINE FIELD name ON user TYPE string;",
    )
    .expect("write schema");
    // `RETURN '<0xFF>';` — valid SurrealQL but for one byte no encoding of
    // UTF-8 produces. The offset it names is where decoding stopped.
    fs::write(root.join("queries/latin1.surql"), b"RETURN '\xff';").expect("write latin-1");
    fs::write(root.join("queries/bad.surql"), "SELECT nope FROM user;").expect("write query");

    let report = check(&discover(&root)).expect("the run is not aborted by one bad file");

    assert!(!report.passed());
    let undecodable: Vec<&str> = report
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("not UTF-8 text"))
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(undecodable.len(), 1, "{:#?}", report.diagnostics);
    assert!(undecodable[0].contains("byte 8"), "{}", undecodable[0]);

    // The good query was still analyzed.
    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.iter().any(|c| c.starts_with("E1002")),
        "the other sources must still be analyzed: {codes:?}"
    );
}
