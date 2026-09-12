# surrealql-analyzer

Static analysis for SurrealQL, as a library. This crate is the layer a host
embeds: it knows what a *project* is — which files on disk are schema, which
are queries, which host files (`.ts`, `.svelte`, …) carry embedded SurrealQL —
and it runs the three verbs a development loop needs over that project.

| verb | what it does |
| --- | --- |
| `check(&project)` | analyze every source; return each finding that survives policy, as data |
| `generate(&project, &options)` | emit the typed TypeScript client + literal-keyed query registry; never overwrite a good registry with a broken one |
| `watch_loop(load_project, extra_inputs, exclude, run)` | re-run a closure on every change to an input the analysis consumes, debounced so one save is one run |

There is **no binary**. SurrealKit owns the command line (`surrealkit check`,
`surrealkit generate`, `surrealkit watch`); the language server
(`surrealql-analyzer-lsp`) owns the editor. Both consume this crate — the LSP
through the re-exported `workspace` engine, SurrealKit through the verbs.

## Embedding

```toml
[dependencies]
surrealql-analyzer = "0.5"
# default-features = false drops the `notify` watcher for a one-shot CI host.
```

A host that already knows its layout builds the config itself; nothing here
needs a `surrealql-analyzer.toml`:

```rust
use surrealql_analyzer::workspace::config::WorkspaceConfig;
use surrealql_analyzer::{check, generate, Project, Styles};

let mut config = WorkspaceConfig::default();
config.sources.schema = vec!["database/schema/**/*.surql".into()];
let project = Project::new(project_root, config);

let report = check(&project)?;
for block in report.render(Styles::new(stderr_is_tty && !no_color)) {
    eprintln!("{block}");
}
if !report.passed() {
    std::process::exit(1); // the host decides what a failure means
}

let out = generate(&project, Some(Path::new("src/db.generated.ts")))?;
println!("wrote {} ({} queries)", out.path.display(), out.queries);
```

`Project::discover(&dir)` walks up for a `surrealql-analyzer.toml` for the
workspaces that have one. `CheckReport::to_json()` is the stable
`{ summary, diagnostics[] }` document; `Diagnostic` is its row.

## Layout

- `project` — the root, the config, and source discovery (globs, ignores,
  schema-before-queries ordering).
- `analyze` — the one pipeline both verbs share: load, analyze, resolve policy,
  and put each finding back at `app.ts:12:5` instead of a registry id.
- `check`, `generate` — the verbs; `diagnostic` — the wire shape of a finding.
- `render`, `style` — rustc-style blocks (excerpt, carets, `help:`), with
  colour as a parameter the host sets.
- `watch` (feature `watch`, on by default) — the `notify`-backed loop.

Engine crates are re-exported as `surrealql_analyzer::{syntax, workspace,
diagnostics, embed, codegen}` so a host depends on one crate.

Licensed under MIT OR Apache-2.0.
