# surrealql-analyzer-codegen

TypeScript type generation from
[SurrealQL Analyzer](https://github.com/surrealdb/analyzer) analysis results.

Three layers. `TypesDocument` describes a project's types once, in a
language-neutral, serializable form: every table with its fields and relations,
every `fn::` signature, every `DEFINE PARAM`, and one entry per analyzed query
carrying its per-statement response kinds and its parameters. `ts_type` renders
one `surrealdb_types::Kind` as a TypeScript type for a named position — an
object member and a standalone value spell `option<T>` differently, and the
caller says which it is. `render_types_module` puts them together and emits the
generated `.d.ts`: an `interface` per table, a `Tables` map, and a `Queries`
type keyed by exact query text.

Types only — the emitted module declares no values and augments no module, so a
consumer parameterises the client with it (`createClient<Queries>(…)`). Value
conventions name what the SurrealDB SDK actually decodes: `record<t>` is
`RecordId<"t">`, `datetime` is `Date`, `uuid`/`duration`/`decimal` are the SDK's
own classes, and `NONE` is an optional key or an `undefined` union member
depending on the position.

The public API is not yet stable; pin an exact version.

## License

MIT OR Apache-2.0.
