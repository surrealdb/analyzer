# surrealql-analyzer-workspace

Workspace, schema, and analysis-orchestration layer for
[SurrealQL Analyzer](https://github.com/surrealdb/analyzer).

This crate is the engine. It holds the source registry and config, extracts the
schema catalog from `DEFINE`/`REMOVE` statements, runs type inference over the
lowered AST, and produces the findings the CLI and the language server consume.
It also answers the editor questions — hover, go-to-definition, completion,
inlay hints — from the same analysis.

Start at the crate root documentation: it names the entry points, the order to
call them in, and the shape of what comes back. In short: build a `Workspace`,
register schema sources before query sources, call `analyze_workspace`, read
the `AnalysisOutput` per source.

The public API is not yet stable; pin an exact version. If all you want is to
check a project, reach for the
[`surrealql-analyzer`](https://crates.io/crates/surrealql-analyzer) CLI.

## License

MIT OR Apache-2.0.
