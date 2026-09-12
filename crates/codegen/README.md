# surrealql-analyzer-codegen

TypeScript type generation from
[SurrealQL Analyzer](https://github.com/surrealdb/analyzer) analysis results.

Two layers: `ts_type` renders one `surrealdb_types::Kind` as a TypeScript type,
and `render_registry` emits the generated module — a literal-keyed registry
mapping each embedded query to its result type, substitution tuple, and
named-parameter object, plus the `defineQuery`/`defineLive` re-exports and the
`SurqlQuery` carrier the host code consumes. Value conventions match the SurrealDB SDK: datetimes are `Date`,
records/uuids/durations are (branded) strings, `NONE` is `undefined`.

**Dormant.** Nothing in the analyzer workspace calls this crate: the module
`render_registry` emits augments `@surrealdb/analyzer-client`, which has left
this repository with the rest of the TypeScript SDKs, and the CLI has no
`generate` verb. `ts_type` — rendering one `Kind` as a TypeScript type for a
named position — is the half of typed-client generation that belongs to the
analyzer, and it is why the crate is kept. The registry half will be rewritten
against whatever the new client's contract turns out to be.

The public API is not yet stable; pin an exact version.

## License

MIT OR Apache-2.0.
