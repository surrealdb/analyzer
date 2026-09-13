# Changelog

## Unreleased

### Fixed — robustness

Six ways a small input could kill, hang, or mislead the analyzer. Every size
below is a measured reproducer on a debug build.

- **Deep nesting aborted the process.** `RETURN ((((…1…))))` 2 000 deep — a
  4 KB file — died with `fatal runtime error: stack overflow`, SIGABRT, exit
  134, and no diagnostic a host could render. So did 2 000 nested arrays,
  `1 + 1 + …` × 2 000, `IF true { … }` × 1 000, and `… AND n > i` × 3 200.
  The CST walks that run before lowering (syntax diagnostics, the
  highlighter) now keep their own stack instead of the call stack, and
  lowering stops at an explicit budget — 128 levels of nesting, 512 terms of
  one left-deep operator chain (whose spine lowering now walks iteratively,
  because generated SQL writes chains hundreds of terms long). Past the
  budget the input raises **one** `6003` hint naming the cut-off and
  everything below it is analyzed as `any`. 50 000 levels now parse, lower,
  highlight and analyze.
- **Nested object literals were exponential.** `RETURN {a:{a:…{a:1}…}}` — 110
  bytes at depth 25 — took 46 s, and depth 30 never finished, because every
  property's type was inferred twice: once for its kind and again for its
  value, doubling the work per level. Each property is inferred once now.
  Depth 25 is 3 ms; depth 40, which was 2^15 times that work, is 3 ms.
- **`AND`/`OR` chains were superlinear.** A `WHERE` with 200 conjuncts took
  9.7 s and 300 timed out. Two causes: lowering a chain to a guard re-ran the
  constant folder over the whole accumulated left operand at every link
  (O(n²)), and the checking walk then asked for that operand's *kind* at
  every link (O(n³)) — a kind `AND`/`OR` never read, since they accept any
  operand. 200 conjuncts is 53 ms; 500 is 0.3 s.
- **One non-UTF-8 byte blinded the whole run.** A single stray byte anywhere
  in the globs aborted with exit 2 and zero diagnostics for the other files.
  That file is now skipped with one `S0001` naming the byte offset, and every
  other source is analyzed. Decoding lossily instead would move every span
  after the bad byte, so the file contributes nothing but the finding.
- **Semantic lints spoke about statements that failed to parse.** `LIVE
  SELECT count() FROM person GROUP ALL` raised `S0001` *and* `W4023` telling
  the reader to add the `GROUP ALL` the statement already has: tree-sitter
  parks the unreadable tail in a node *beside* a statement that otherwise
  lowers cleanly, so the truncated statement was analyzed as if the tail were
  not written. A finding in the same `;`-delimited statement as a syntax
  error is now dropped; its well-formed neighbours keep theirs.
- **One parse failure raised two `S0001`s.** `RELATE 'user:1' -> wrote ->
  post:1;` reported the whole statement and the operand inside it. `ERROR`
  nodes nest; only the innermost — the one that names the offending text —
  is reported now.

### Fixed — a watch could re-trigger itself forever

`resolve_output` canonicalized the registry's *parent* to get the spelling the
watcher reports. When that parent does not exist yet — the first `generate`
creates it — canonicalization failed and the raw path was kept, so under a
symlinked ancestor (`/tmp`, `/var`, a symlinked home) the path written and the
path excluded never matched: `generate` wrote, the watcher called the write an
input, and the loop ran continuously. Resolution now starts at the nearest
ancestor that does exist and re-appends the rest.

### Changed — `source` in the JSON document is relative to the project root

A `Diagnostic`'s `source` was whatever the source id happened to be: a bare
absolute path for a finding in a host file, `file:///abs/...` for one in a
`.surql` file, and always `file://` for `related[].source` — while the same
finding rendered as text said `src/app.ts`. Every one of them is now the
root-relative path the renderer shows, so a consumer need not know which kind
of source it is holding, and two machines analyzing the same commit produce the
same document. A file outside the root keeps its absolute path.

### Fixed — `generate` creates the directory it writes the registry into

`generate` wrote the registry with a bare `fs::write`, so an `out` naming a
directory that does not exist yet — `src/lib/db.generated.ts` in a project
without a `src/lib/`, the shape the documentation uses — failed with an ENOENT
that named the *file*, not the missing parent. The parent is created first now.

### Fixed — three places the grammar accepted syntax the engine does not

Each was established against a live SurrealDB 3.2.3 and against the version
history in surrealdb/surrealdb, and the three histories came out differently
— so the three fixes are different.

- **`RATELIMIT` is removed from the grammar.** `DEFINE TABLE`/`DEFINE FIELD`
  parsed a `RATELIMIT FOR <action> … LIMIT n PER <duration>` clause that
  exists in no release and in no commit of SurrealDB on any branch: a pickaxe
  over every ref finds only HTTP transport rate limiting. 3.2.3 does not even
  lex the word as a keyword. Nothing for 8002 or 8003 to name, so the rule
  goes.
- **`PARALLEL` keeps parsing and now reports E8002.** The clause was real —
  every 1.x and 2.x release takes it on all seven of SELECT/CREATE/UPDATE/
  UPSERT/DELETE/RELATE/INSERT — and was removed in 3.0.0 by surrealdb#6768 as
  a no-op. A 2.x user migrating gets the removal named instead of a token
  error that collapses the file. Reported when `analysis.surrealdb_version` is
  3.0 or newer.
- **`INSERT RELATION IGNORE` parses, and the reverse reports the new E4030.**
  The engine takes `RELATION` before `IGNORE` and always has; the grammar had
  the two backwards, so the valid spelling failed to parse and the invalid one
  passed silently. Both orders parse now, and the reversed one is reported
  with the order to write instead.

### Changed — the analyzer is a library; the command line is SurrealKit's

`surrealql-analyzer` no longer ships a binary. The crate of that name is now
the library a host embeds — `Project`, `check`, `generate`, and the
`notify`-backed `watch_loop` — and the `check`/`generate`/`watch` verbs are
[SurrealKit](https://github.com/surrealdb/surrealkit)'s to spell
(`surrealkit check`, `surrealkit generate`, `surrealkit watch`). The language
server is unchanged and remains the one binary this repository releases.

- **Crate:** `crates/cli` → `crates/analyzer`, still published as
  `surrealql-analyzer`. `clap`, the spinner and the terminal layout are gone;
  what moved down is everything a host would otherwise reimplement: source
  discovery, embedded-query collection, host-span remapping, policy
  resolution, the JSON document, the rustc-style renderer, and the watcher.
- **Verbs return data.** `check` returns `Ok(CheckReport)` whether or not it
  found errors — `report.passed()` decides an exit code — and only a run that
  could not happen is `Err`. `generate` returns `GenerateReport` or
  `GenerateError::Blocked` (an error in an embedded query; nothing written).
  Rendering is a separate, opt-in call with colour as a `Styles` parameter.
- **No config file required.** `Project::new(root, WorkspaceConfig)` lets a
  host that knows its own layout build the config directly;
  `Project::discover(dir)` still walks up for a `surrealql-analyzer.toml`.
- **Watch is engine-only.** `watch_loop` decides *when* and *why*
  (`WatchRun { index, reason, changes }`); the host's closure decides what a
  run does and how it is shown. The project comes from a `load` closure, so a
  host with a fixed config passes a clone and one with a file re-reads it, and
  the host names its own config file as an extra input to watch.
- **Removed:** the `surrealql-analyzer` npm launcher, the `cargo-binstall`
  metadata, the CLI half of the release workflow, and `surrealql-analyzer init`
  (the `[sources]` layout is SurrealKit's to know). `scripts/oracle.py` runs the
  corpus through `cargo run --example check_json` instead of a binary.

### Added — diagnostics found by probing, verified on SurrealDB 3.2.3

- **E4031** — a payload `id` that disagrees with the statement's record target
  (`CREATE p:1 CONTENT { id: p:2 }`); the engine refuses the write outright.
- **W4032** — `ORDER BY`/`LIMIT`/`START` on a single record id: `START` skips
  the only row and the statement returns nothing.
- **W7006** now covers the set operators handed a scalar (`tags CONTAINSANY
  'x'`): the ANY/ALL forms are always false, the NONE forms always true.
- **E1033** now covers `REFERENCE` on a non-record type, which the engine
  rejects.
- **E1012** now covers the analyzer a `FULLTEXT`/`SEARCH ANALYZER` index names —
  the engine accepts the definition and fails only on first search.
- **E8002** now covers `MTREE` and the bare `<|k|>` KNN operator (with a
  configured 3.x target); E1027's message names HNSW/FULLTEXT instead.
- **W7012** reaches `DEFINE EVENT … THEN` bodies (blocking calls and `SLEEP`).

### Fixed

- `sleep(1s)` no longer reports E5001: the builtin was registered only under
  `sleep::sleep`, a spelling SurrealQL does not have.

### Changed — the project is now the SurrealQL Analyzer

SurrealGuard has been renamed to the **SurrealQL Analyzer** and moved to
`github.com/surrealdb/analyzer`. Nothing about the analysis changed; every name
did. There is **no fallback to the old names** — this is a clean break.

- **Crates:** `surrealguard` → `surrealql-analyzer`, and each
  `surrealguard-<part>` → `surrealql-analyzer-<part>` (`-syntax`, `-workspace`,
  `-diagnostics`, `-lsp`, `-codegen`, `-embed`, `-macros`, `-rs`, `-wasm`,
  `-tree-sitter-surrealql`). Rust paths follow: `surrealguard_syntax` →
  `surrealql_analyzer_syntax`. The grammar crate still publishes under a
  `package` rename, so `tree_sitter_surrealql` stays its importable name.
- **Binaries:** `surrealguard` → `surrealql-analyzer`, `surrealguard-lsp` →
  `surrealql-analyzer-lsp`. Release archives and the `npx` launcher follow the
  same names.
- **Config file:** `surrealguard.toml` → `surrealql-analyzer.toml`. The old
  filename is no longer discovered by the CLI, the language server, or the
  TypeScript plugin — rename the file.
- **npm:** the `@surrealguard/*` scope → `@surrealdb/analyzer-*`
  (`@surrealdb/analyzer-client`, `-query`, `-next`, `-svelte`, `-ts-plugin`),
  and the `surrealguard` CLI shim → `surrealql-analyzer`.
- **TypeScript API:** `SurrealGuardClient` → `SurrealQLAnalyzerClient`,
  `SurrealGuardError` → `SurrealQLAnalyzerError`, `SurrealGuardProvider` →
  `SurrealQLAnalyzerProvider`, and the Svelte preprocessor `surrealguard()` →
  `surrealqlAnalyzer()`.
- **Generated output:** the default filename is now
  `surrealql-analyzer.generated.ts`, with a `// Generated by surrealql-analyzer`
  header.
- **Suppression comments:** `-- surrealguard: allow(E1001) reason="…"` is now
  `-- surrealql-analyzer: allow(E1001) reason="…"`.
- **Environment variables:** `SURREALGUARD_SCHEMA` → `SURREALQL_ANALYZER_SCHEMA`,
  `SURREALGUARD_TEST_WS` → `SURREALQL_ANALYZER_TEST_WS`.

Two large bodies of work met here: an August series on the SurrealQL grammar,
LIVE SELECT and the Svelte/embedded-query surfaces, and a correctness and
performance sweep over the engine, the language server and the test suite.
Every rule below that concerns SurrealDB's own behaviour was verified against a
live 3.2.3 server rather than inferred.

### Fixed — the language server no longer grows without bound

Startup and every save copied every document's text once per document and held
all the copies alive at once. On a 509-file workspace that was 887 MB of
resident memory that never came back; it is now 77 MB and flat across saves.
Document text is shared rather than copied, diagnostics publish per document,
and the workspace scan skips build directories and honours the globs in
`surrealql-analyzer.toml`.

Two smaller leaks went with it: the semantic-tokens refresh was re-requested on
every publish with no capability gate and no coalescing, so a client that never
answered accumulated pending requests forever; and the server ignored the `exit`
notification, ending only on end-of-input, so an editor that restarted the
server could leave the old process running.

### Fixed — editor latency on large files

Converting a byte offset to an editor position rescanned the document from the
first byte, once per span. It is now a cached per-document line index. On a
3,200-line file, diagnostic conversion went from 475 ms to 2.2 ms and whole-file
inlay hints from 253 ms to 15.6 ms, and the cost per line is flat across file
sizes instead of growing.

### Added — diagnostics

Four new contracts, each engine-verified: a constant written to a field that
violates that field's own `ASSERT` (2038); an aggregate over a column with no
`GROUP` clause, which the engine rejects (4028); a projection under `GROUP BY`
that is neither a group key nor an aggregate, which the engine silently
accumulates into an array (4029); and a parameter read by a `DEFINE FUNCTION`
body that nothing binds (6008), which evaluates to `NONE` and quietly computes
the wrong answer.

Version compatibility (8001, 8002, 8003) now emits, keyed on an optional
`[analysis] surrealdb_version`. Unset means the latest release and nothing
fires. The registry of when each function and syntax feature appeared or was
removed is sourced from SurrealDB's own tags and documentation.

Completeness for contracts that previously covered only part of their surface:
duplicate and missing-target reporting for `INDEX`, `EVENT`, `FUNCTION`,
`PARAM` and `ANALYZER`; `REMOVE` and `ALTER` now update the schema so later
statements see the change; event self-trigger cycles (5010); `OMIT` without a
wildcard projection (4012); and the side-effect-in-computed-field lint widened
beyond `http::`.

Retired, having never had an emission site: 4001, 4002, 4016 and 6006. Retired
numbers are not reused.

### Added — grammar and conformance

SurrealDB's own extracted test queries now parse completely: 320 of 320, with a
further 8 entries that must *not* parse (parser error-handling fragments and a
form the engine reserves) asserted as rejected, so the harness catches
over-acceptance as well as regression. The upstream `surrealql-tree-sitter`
corpus passes 396 of 396.

Newly parsed, each verified against the engine: `%`; prefix `NOT`, `-` and `+`
on any operand; `dec` and `f` numeric suffixes; `s''` strings; `?.`; `WITH
INDEX` on mutations; `INSERT IGNORE`; `KILL $param`; `LIVE SELECT ... FROM
$param`; `array<T, N>`; `COMMENT` clauses; `HNSW`, `DISKANN`, `FULLTEXT` and
`COUNT` index kinds; `RATELIMIT`; `DEFINE SEQUENCE`; `DEFINE ACCESS` and
`DEFINE USER`; the `ACCESS` statement; `EXPLAIN` in prefix position; an
immediately-called closure; and `ORDER BY count`.

Lowering gaps that silently hid bugs are closed: every comparison and
containment operator has its own typed variant and infers `bool`, where the
whole family was previously untyped and unchecked; `=` and `==` are
distinguished; `IF NOT EXISTS` no longer reads as a duplicate definition;
`INSERT ... ON DUPLICATE KEY UPDATE` keeps its row payload; `LIVE SELECT`
lowers its `WHERE` and `FETCH`; and `r'table:id'` is a record id rather than a
regex.

### Changed — internals

One field-projection policy replaces five implementations that disagreed on
`option`, unions and record links; joins route through the lattice; one AST
visitor replaces three hand-rolled walkers; the builtin catalogue is one
structured table that both dispatches calls and drives completion; and host-file
span remapping is one method shared by the CLI, the language server and the
WASM host.

### Added — tests

Whole-pipeline tests moved out of `analysis.rs`, which fell from 6,568 lines to
882. The corpus gained 426 engine-verified statements over previously untested
surface. The negative corpus went from 4 distinct codes at 0.5.3 to 81. Every
deny-level code has a fire test and a near-miss guard. The parser gained
property-based testing over corrupted input, which found a missing identifier
being treated as a real empty name across sixteen lowering sites. Generated
TypeScript is compared against a committed golden and typechecked.

### Known

A full edit-to-diagnostics cycle is superlinear in document size: roughly 63 µs
per line at 200 lines and 752 µs at 3,200. This is the analysis pipeline, not
span conversion — a document with no findings costs the same. Under
investigation.

## 0.4.0

The headline is **type-aware autocomplete**. Alongside it, a large correctness pass:
every rule below that concerns SurrealDB's own behaviour was verified against a live
SurrealDB 3.0.5 server rather than inferred, and several turned out to contradict what
the analyzer previously assumed.

### Breaking — generated types change shape

If you consume generated TypeScript, expect these keys/types to differ. Each is a fix
toward what the engine actually returns.

- **Unaliased call projections are keyed by the bare function name.** `SELECT fn::abc(age)`
  is `{ "fn::abc": … }`, not `{ "fn::abc(age)": … }`; likewise `string::len(name)` →
  `string::len`, `time::now()` → `time::now`. An idiom ending in a method drops the method
  (`name.len()` → `name`). Everything else keeps its source text (`age + 1`,
  `math::abs(age) + 1`). Previously the raw source text was used as the key, so consumers
  indexed a key the runtime never returns and read `undefined`.
- **Rows carry the implicit `id`** (and `in`/`out` on RELATION tables) in wildcard
  projections and every mutation return. Schemas rarely declare `id` explicitly, so most
  row types previously omitted their primary key.
- **`type::field` / `type::fields` expand** into the fields their path arguments name,
  matching the engine.
- **A `.{ … }` destructure over a wrapped link hoists the wrapper to the object**:
  `members.{name}` over `array<record<user>>` is `array<{ name: string }>`, not
  `{ name: array<string> }`.

### Added

- **Type-aware completion** (`completion_provider`). Fields, tables, `$params`, `fn::` and
  builtin paths, `.` members across record links, `.{ }` destructure members, `CONTENT`
  keys, and method sugar. Ranking combines fuzzy matching with **type compatibility as a
  primary signal**, so a param whose kind fits the position outranks a closer name match.
  Served entirely from the analysis cache — no re-analysis or re-parse per keystroke.
  - Graph slots offer only what the receiver can actually traverse, direction-aware and
    multi-hop; `GROUP BY` offers only what can label a group; a graph step's filter
    completes the step's own fields; `WITH INDEX` offers that table's indexes, and
    full-text functions are withheld where the schema proves no such index exists.
- **New diagnostics**: `4025` (a wildcard under `GROUP` — the engine rejects the query),
  plus `4013`, `6002` and `7001` (opt-in), which were catalogued but never emitted.
- `check` now scans host files (`.ts`/`.tsx`/`.svelte`/…), so embedded queries are covered
  by CI. Previously a workspace whose `generate` failed could pass `check` with exit 0.

### Fixed

- **`??` strips `NONE` from an option left operand.** `(optional ?? 'default') + '!'` raised
  a false `E2004` whose help told you to coalesce — which you had.
- **Method dispatch** resolves what the engine accepts (`to_string`, `type_of`, `diff`,
  `patch`, `repeat`, the whole `is_*` family). An unresolved method is reported as `E5001`,
  so these were error-severity false positives that aborted `generate` for a whole project.
- **`GROUP BY` / `ORDER BY` over a projection alias** no longer reports a false `E1002`.
- **Aggregate columns** promote for any argument expression and at any depth in a
  computation, instead of only a bare `math::x(field)`.
- **An opaque `CONTENT $payload`** no longer claims every required field is missing.
- **Descendant field definitions refine the parent's kind rather than replacing it**, so
  `array<object>` + `[*].price` and `option<object>` + subfields keep their declared shape
  (and their `DEFAULT`). Paths into a refined parent still resolve.
- **Wrapped record links traverse**: `option<record<T>>`, `array<record<T>>` and `set<…>`
  resolve through the link and keep their wrapper, instead of degrading to `unknown`.
- **Array-form `INSERT` payloads are checked.** `INSERT INTO t [{ … }]` was discarded by the
  lowerer, so required fields, unknown keys and value kinds went entirely unvalidated.
- **A wildcard under `GROUP` no longer fabricates** the non-grouped fields, which promised
  fields that do not exist at runtime.
- `COMPUTED`/`VALUE` fields reading a sibling `$this.<field>` no longer degrade to `any`.
- `FOR` over a union-wrapped collection types its loop binding; index/method operations
  distribute over a union; an empty-array `RETURN` no longer widens a block's return type.
- Writing a wrong literal to a literal-union field errors again, naming the value.
- `check --json` keeps warnings on a clean run (the summary counted them; the array was empty).

### Performance

- **LSP incremental analysis.** A query-file edit re-analyzes only that file against a
  cached schema (~96× on the measured corpus). A schema edit uses symbol-level
  invalidation — a function-body edit re-analyzes the edited file plus only what
  references a changed symbol, rather than the whole workspace (1.70s → 256ms on a
  149-file corpus; ~30ms per edit in practice).
