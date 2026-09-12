<div align="center">

# 🛡️ SurrealQL Analyzer

### Static analysis and type inference for SurrealQL

Catch unknown fields, kind mismatches, and bad graph traversals *before* a query
reaches SurrealDB.

[![crates.io](https://img.shields.io/crates/v/surrealql-analyzer?label=surrealql-analyzer&color=e07b39&logo=rust&logoColor=white)](https://crates.io/crates/surrealql-analyzer)
[![CI](https://github.com/surrealdb/analyzer/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/surrealdb/analyzer/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-3b82f6)](#license)

[**Docs**](https://surrealguard.dev/docs/) · [**Live playground**](https://surrealguard.dev/#playground) · [**DESIGN.md**](docs/DESIGN.md)

</div>

---

## What it is

SurrealQL Analyzer parses your `.surql` schema and queries into a typed, span-carrying
AST, infers the response type of every statement, and reports violations of each
construct's contract. One engine, three front ends:

| | |
| --- | --- |
| **CLI** — `surrealql-analyzer` | `check` your workspace in CI or `watch` it while you develop |
| **Language server** — `surrealql-analyzer-lsp` | Diagnostics, hover, inlay hints, go-to-definition, and type-aware completion, in `.surql` files *and* in SurrealQL embedded in TypeScript / Svelte / Vue / Astro |
| **Rust library** — `surrealql-analyzer-workspace` | The engine itself: build a workspace, analyze it, read back findings and inferred types. This is what the two binaries are built on, and what yours can be |

> **The client SDKs have moved out.** `@surrealdb/analyzer-{client,query,next,svelte}`,
> the TypeScript language-service plugin and the `query!` / `surql!` Rust macros
> used to live here. They are client libraries, not analysis, and they are being
> rehomed; the published packages on npm and crates.io are unaffected. This
> repository is the analyzer: one binary, one language server, one library.
> Typed-client generation will return once it is designed against the new client.

## Why

SurrealDB is permissive at runtime: cross-kind comparisons order by kind instead
of failing, coercions succeed silently, and a misspelled field just returns
`NONE`. That is exactly where bugs hide. SurrealQL Analyzer is **contract-first** —
every construct has a contract (what the author must mean for the statement to
make sense), and the analyzer reports violations of that contract even when the
engine would happily execute the query.

SurrealDB's own parser discards spans before producing its AST, so it cannot
power an analyzer or editor tooling. SurrealQL Analyzer parses with tree-sitter into a
typed, span-carrying AST and runs all analysis on that.

## Quickstart

```sh
npm i -D surrealql-analyzer         # or: cargo binstall surrealql-analyzer
npx surrealql-analyzer init         # writes a commented surrealql-analyzer.toml
```

Point `surrealql-analyzer.toml`'s `schema` glob at your `.surql` files, and write a
schema:

```surql
-- schema/schema.surql
DEFINE TABLE team SCHEMAFULL;
DEFINE FIELD name ON team TYPE string;

DEFINE TABLE person SCHEMAFULL;
DEFINE FIELD name ON person TYPE string;
DEFINE FIELD age ON person TYPE int;
DEFINE FIELD team ON person TYPE record<team>;
```

Queries are checked wherever they live — a `.surql` file matched by the
`queries` glob, or an ordinary string literal in your own code:

```ts
// src/main.ts
const [people] = await db.query("SELECT name, ag FROM person WHERE team = $team", {
  team: new RecordId("team", "red"),
});
```

```sh
npx surrealql-analyzer check
```

```
error[E1002]: `person` has no field `ag`
  --> src/main.ts:2:44
  |
2 | const [people] = await db.query("SELECT name, ag FROM person WHERE team = $team", {
  |                                            ^^
  |
  = help: did you mean `age`?
```

While you are developing, run it as a loop instead — `watch` re-checks the whole
workspace on every save:

```sh
npx surrealql-analyzer watch        # the same as `check --watch`
```

## Check in CI

```sh
npx surrealql-analyzer check
```

`check` analyzes both your `.surql` files and the SurrealQL embedded in host
files, and reports findings rustc-style at their real `file:line`:

```
error[E1002]: `person` has no field `ag`
  --> src/routes/+page.svelte:6:39
  |
6 |   const rows = db.query("SELECT name, ag FROM person");
  |                                       ^^
  |
  = help: did you mean `age`?
note: `person` is defined here
  --> schema/schema.surql:4:14
  |
4 | DEFINE TABLE person SCHEMAFULL;
  |              ^^^^^^

checked 3 sources in 6ms
found 1 error
```

On a terminal the severity, code, paths and carets are coloured; piped to a file
or a CI log it is the same text with no escape sequences, and `--no-color` /
`NO_COLOR` turn colour off explicitly.

The exit code reflects the post-policy error count, so it drops straight into
CI. `--json` emits `{ summary, diagnostics[] }` with byte-offset ranges for
tooling — one document, one exit code, never decorated.

## Use it as a library

The CLI and the language server are two consumers of one library, and it is
published for a third. `surrealql-analyzer-workspace` is the engine:

```rust
use surrealql_analyzer_workspace::{analyze_workspace, Workspace, WorkspaceConfig};

let mut workspace = Workspace::new(WorkspaceConfig::default());
workspace.add_virtual_source(
    "schema.surql".into(),
    "DEFINE TABLE person SCHEMAFULL; DEFINE FIELD age ON person TYPE int;".into(),
);
let query = workspace.add_virtual_source("q.surql".into(), "SELECT age FROM person;".into());

let analysis = analyze_workspace(&workspace);
for finding in &analysis.diagnostics {
    println!("{} {}", finding.code(), finding.message());
}

// Per source: findings, one record per statement, the inferred params, the
// `LET` bindings, and the response kind when exactly one statement responds.
let output = &analysis.sources[&query];
assert!(output.response_kind.is_some());
```

Schema sources must be registered before the queries that reference them —
`add_file_source` and `add_virtual_source` preserve registration order, and the
schema index is built in that order. The crate root documents the whole entry
sequence; the query-level helpers the language server calls (`hover_at`,
`definition_at`, `complete_at`, `let_binding_hints`) hang off the same analysis.

`surrealql-analyzer-diagnostics` owns the finding codes, their severities and
the lint policy that grades them; `surrealql-analyzer-syntax` owns the parser
and the span-carrying AST; `surrealql-analyzer-embed` finds SurrealQL inside
TypeScript and Svelte files. Each is usable on its own.

## Editors

`surrealql-analyzer-lsp` serves the analyzer over stdio for `.surql` files and for
SurrealQL embedded in host files, so squiggles land on the exact token inside an
inline query. It provides diagnostics, **type-aware completion** (fields, tables,
`$params`, `fn::` and builtin paths, graph steps that only offer traversable
edges), **inlay type hints** on `LET` bindings, **hover** on tables, `LET`
variables and context params, and go-to-definition. It reads `surrealql-analyzer.toml`,
so your `[lints]` levels apply live in the editor.

Zed users can install the
[`DrewRidley/zed-surreal`](https://github.com/DrewRidley/zed-surreal) extension;
any other LSP-capable editor can point at the binary directly.

## What it analyzes

- **Full statement coverage** — SELECT (projections, graph traversals,
  FETCH/SPLIT/GROUP/OMIT), the six mutations, RELATE, LET/RETURN/IF/FOR/blocks,
  transactions, DEFINE/REMOVE/ALTER, LIVE SELECT/KILL, and the rest.
- **Type inference** — response kinds as upstream `surrealdb_types::Kind`: closed
  object literals for known rows, unions from IF/ELSE, record-link and graph-edge
  shapes, the full builtin function table plus `fn::` declarations, closure and
  subquery inference, and constant-value evaluation.
- **A contract catalog of 87 diagnostics** in families (1xxx schema references,
  2xxx types, 3xxx graph, 4xxx statement misuse, 5xxx functions, 6xxx parameters,
  7xxx lints). Severities are intrinsic to each
  finding; consumers apply policy (warnings-as-errors, lint levels) at their edge,
  rustc-style. Messages lead with the consequence and attach `help:` fixes and
  `note:` spans pointing at the relevant definition.
- **Flow-sensitive checks** — control-flow type narrowing (occurrence typing that
  narrows `option<T>` and `record<A|B>` unions via `= NONE` guards and
  `type::table()` discriminants, field paths included), PERMISSIONS-predicate
  analysis, comparison footguns (`= NONE`/`= NULL` on a kind that excludes it,
  `IN`/`CONTAINS` element-kind mismatch, disjoint record-link equality), aggregate
  `count()` without `GROUP`, `record<UndefinedTable>`, `DEFAULT` violating a
  field's own `ASSERT`, and unreachable code.
- **Parameter constraints** — every `$param` a source reads is exported with the
  kind and value domain its uses imply (`UPDATE user SET age = $age` → `age: int`;
  `LIMIT $n` → non-negative int).
- **Byte-precise spans** on every finding, for editor squiggles.

## Configuration

`surrealql-analyzer.toml` is discovered by walking up from the working directory; the
directory holding it is the workspace root. `surrealql-analyzer init` writes a fully
commented starter.

```toml
[sources]
schema = ["schema/**/*.surql"]     # DEFINE/REMOVE catalog; analyzed first
queries = ["queries/**/*.surql"]   # analyzed against that schema
ignore = ["node_modules/**", "target/**"]

[analysis]
strict = false

[diagnostics]
warnings_as_errors = false

# "allow" | "warn" | "deny". A specific code beats a family wildcard.
[lints]
# E1002 = "allow"
# "7xxx" = "warn"
```

Host files (`.ts`, `.tsx`, `.js`, `.jsx`, `.svelte`, `.vue`, `.astro`) are
scanned for embedded queries under the same `ignore` patterns — they need no
glob of their own.

## Install

**CLI, from npm:**

```sh
npm i -D surrealql-analyzer            # as a project dev dependency
```

The `surrealql-analyzer` npm package is a launcher that downloads the prebuilt binary
matching its version. `npx surrealql-analyzer --help` works without installing.

**CLI and language server, from crates.io:**

```sh
cargo binstall surrealql-analyzer      # prebuilt; `cargo install surrealql-analyzer` builds it
cargo install surrealql-analyzer-lsp   # the language server
```

**The library:**

```sh
cargo add surrealql-analyzer-workspace surrealql-analyzer-diagnostics
```

Prebuilt archives for both binaries, every supported target, are attached to each
[GitHub Release](https://github.com/surrealdb/analyzer/releases).

## Project layout

```
surrealql-analyzer/
├── crates/
│   ├── syntax/                # tree-sitter parsing, typed span-carrying AST, lowering
│   ├── tree-sitter-surrealql/ # the vendored SurrealQL grammar
│   ├── diagnostics/           # finding types, code catalog, severity/lint policy
│   ├── workspace/             # schema index, analyzers, inference, analysis pipeline
│   ├── embed/                 # embedded-SurrealQL extraction from host files
│   ├── codegen/               # Kind → TypeScript generation (dormant; see below)
│   ├── cli/                   # the `surrealql-analyzer` binary
│   ├── lsp/                   # the `surrealql-analyzer-lsp` binary
│   └── wasm/                  # the browser-playground analyzer build
├── web/               # the static site, incl. the generated diagnostics catalogue
├── npm/               # the `surrealql-analyzer` npx launcher
└── docs/              # DESIGN.md + design plans (incl. the diagnostic catalog)
```

`crates/codegen` is dormant. It renders a `surrealdb_types::Kind` as a
TypeScript type, which is the half of typed-client generation that belongs to
the analyzer; the module it currently emits augments a client package that no
longer lives here, so nothing drives it and the CLI has no `generate` verb. It
is kept as the seed of the redesign rather than rewritten speculatively.

The grammar is vendored in-repo at `crates/tree-sitter-surrealql`, copied
verbatim from the official
[`surrealdb/surrealql-tree-sitter`](https://github.com/surrealdb/surrealql-tree-sitter)
(SurrealQL Analyzer's precedence fix and feature additions were merged there upstream).

## Contributing

`cargo test --workspace` runs the suite; `cargo clippy --workspace
--all-targets` with `RUSTFLAGS="-D warnings"` is the CI gate; `cargo fmt --all`
formats. The one Node step is the published diagnostics catalogue, regenerated
from `crates/diagnostics/src/catalog.rs` with `pnpm docs:diagnostics` and gated
in CI. [`AGENTS.md`](AGENTS.md) documents the precision harnesses (snapshot,
`any` ratchet, real-binary LSP tests) that guard against silent regressions, and
[`docs/DESIGN.md`](docs/DESIGN.md) covers architecture and roadmap.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Unless you explicitly state otherwise,
any contribution intentionally submitted for inclusion in this project by you, as
defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
