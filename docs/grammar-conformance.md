# Grammar conformance report (2026-09-11)

**Both corpora are at 100%.** There is no expected-failure baseline any more;
`crates/syntax/tests/conformance_expected_failures.txt` is gone.

| corpus | entries | result |
| --- | --- | --- |
| valid set (`crates/syntax/examples/conformance_corpus.json`) | 362 | 362 parse, 0 fail |
| rejected set (`crates/syntax/examples/conformance_rejected.json`) | 18 | 18 refused, 0 wrongly accepted |
| upstream `surrealql-tree-sitter` `test/corpus/*.txt` (head `22feaab`) | 396 | 396 parse |

(319/11 was this file's count as of the RATELIMIT/PARALLEL/INSERT-order work;
`THROW`-as-expression, the `SET` bracket target, `PATCH` as any expression,
and `LIMIT`/`START`/`TIMEOUT` as expressions each added to the valid set
without a doc update in between — 334/11 immediately before this round.)

Both sets are extracted from SurrealDB's own test suites, except for eight
entries (seven valid, one rejected) added here to pin the `ORDER BY count`
work and the INSERT modifier order below. Those eight were each run against
a live 3.2.3 first; an entry that is not from SurrealDB's suites earns its
place by engine evidence, not by assertion. Three entries that claimed to
come from those suites did not — see "RATELIMIT was never SurrealQL".

Runner: `cargo run -p surrealql-analyzer-syntax --example conformance`
(add a path to check one file as a valid set only).
Gate: `cargo test -p surrealql-analyzer-syntax --test conformance`.

## The valid/rejected split

The corpus used to be one list of "known-valid SurrealQL" held against a
baseline of entries that still failed. That conflated two very different
things, and the baseline hid both: a real grammar gap and a string that was
never SurrealQL sat on the same line, and the only thing the gate could say
was "still failing".

There are now two committed corpora, and the gate fails in **both**
directions:

- **The valid set** — `conformance_corpus.json`, a JSON array of query
  strings. Every entry must parse cleanly. A parse error is fatal to the
  whole source (the analyzer sees a `Partial` statement and says nothing),
  so an entry the grammar rejects is a place the analyzer is silently wrong
  on valid input. **Fix the grammar, never the corpus.**
- **The rejected set** — `conformance_rejected.json`, a JSON array of
  `[query, reason]` pairs. Text that came out of the same test suites but is
  not SurrealQL. Every entry must fail to parse. An entry that starts parsing
  is **over-acceptance**: the grammar grew looser than the language, and text
  SurrealDB itself refuses would reach the analyzer as if it were a real
  query. That check did not exist before.

Every rejected entry carries a reason, a test asserts none is empty, and a
test asserts the two sets are disjoint. There is no `UPDATE_SNAPSHOTS` path:
at 100% in both directions the invariant is absolute, so there is nothing to
re-record. Moving an entry between the sets is a deliberate edit with a
reason, not a regeneration.

## What the 20 former failures actually were

The 2026-09-08 report listed 21 failures (one, `#85`, had since been banked)
and called them all "junk extractions". They were three different things.

**Thirteen were valid SurrealQL the extractor mangled** by keeping a Rust
source line-continuation — a trailing `\` (or, for two entries, a literal
two-character `\n`). Each was cross-checked against SurrealDB's own sources
at rev `c7eac9022` and repaired to what SurrealDB actually tests:

| # | source | repair |
| --- | --- | --- |
| 169 | `core/tests/define.rs:553` | strip trailing `\` |
| 177 | `core/tests/define.rs:724` | strip trailing `\` |
| 180 | `core/tests/define.rs:774` | strip trailing `\` |
| 186 | `core/tests/remove.rs:994` | strip trailing `\` |
| 189 | `core/tests/remove.rs:1079` | strip trailing `\` |
| 192 | `core/tests/define.rs:1086` | strip trailing `\` |
| 198 | `core/tests/define.rs:1165` | strip trailing `\` |
| 203 | `core/tests/index.rs:237-239` | collapse the continuations (see below) |
| 242 | `core/tests/complex.rs:134` | strip literal `\n` |
| 243 | `core/tests/complex.rs:135` | strip literal `\n` |
| 291 | `core/tests/define.rs:811` | strip trailing `\` |
| 292 | `core/tests/define.rs:905` | strip trailing `\` |
| 293 | `core/tests/define.rs:994` | strip trailing `\` |

Entry 203 is the interesting one: a `\` before a newline in a Rust string
literal eats the newline *and the next line's leading whitespace*, so the
string SurrealDB parses is the three statements run together with no
separator at all —
`INFO FOR INDEX field1 ON aaa;count(…);SELECT VALUE [field1, count] FROM (…)`.
The corpus now holds that.

Twelve of the thirteen parsed as soon as the continuation was stripped: the
gaps the old report implied (`DURATION FOR TOKEN …, FOR SESSION …`,
`PERMISSIONS FULL` on `DEFINE PARAM`/`FUNCTION`, `DEFINE EVENT … THEN
RETURN`, `INFO FOR INDEX … ON`) had all been closed already, and only the
backslash was keeping them red. Entry 203 exposed **one real grammar gap**:
`count` was a function-name token everywhere, so it could not be a field
name. `SELECT field1, count() FROM t GROUP field1` names its aggregate
column `count`, and `SELECT VALUE [field1, count] FROM (…)` reads it back —
verified on 3.2.3. `count` joined `_nonReservedIdent` beside
`order`/`start`/`limit`/`group`/`key`.

**Three were regex assertions, not queries** (#283, #285, #287):
`DEFINE USER user ON <level> PASSHASH .* ROLES VIEWER`, where `.*` matches
the password hash in `INFO` output (`core/tests/info.rs:104/119/134`). The
real *input* queries behind them live a few lines above
(`core/tests/info.rs:89-91`) and were restored to the valid set:
`DEFINE USER user ON ROOT|NS|DB PASSWORD 'pass';`. The `.*` strings moved to
the rejected set.

**Four were deliberate fragments** from SurrealDB's parser *error-handling*
tests — `}`, `a:[`, `]`, `SELECT * FROM` (#130, #131, #133, #244). They are
meant not to parse, so they moved to the rejected set. Making the grammar
accept them would have made us wrong.

Net for this step: 319 entries → 315 valid (4 fragments out) + 7 rejected
(4 fragments + 3 regex assertions), with 3 restored queries taking the place
of the regex assertions. The six `ORDER BY count` entries added afterwards
brought the committed sets to 320 and 8; the three findings recorded below
moved them to their current 319 and 11.

## Where the grammar is deliberately looser than the engine

Three upstream corpus cases are forms SurrealDB 3.2.3 refuses outright:

| form | engine error | diagnosed as |
| --- | --- | --- |
| `KILL "plain-string"` | ``Unexpected token `a strand`, expected a UUID or a parameter`` | E2020 |
| `SHOW CHANGES FOR TABLE person` | ``Unexpected token `;`, expected SINCE`` | E2021 |
| `SHOW CHANGES FOR TABLE person LIMIT 10` | ``Unexpected token `LIMIT`, expected SINCE`` | E2021 |
| `INSERT IGNORE RELATION INTO likes {…}` | ``Unexpected token `INTO`, expected Eof`` (``…`{`…`` without the optional `INTO`) | E4030 |
| `SELECT … PARALLEL` and the six other statements that took the clause | ``Unexpected token `PARALLEL`, expected Eof`` | E8002, with a target |
| `LIVE SELECT … ORDER BY`/`GROUP`/`LIMIT`/`START`/`SPLIT`/`OMIT`/`TIMEOUT`/`PARALLEL`/`EXPLAIN`/`FROM ONLY`/two tables | ``Unexpected token `ORDER`, expected Eof`` (and so on, one per clause) | E4009 |
| `$x = 1;` with no `LET` | `` Parameter declarations without `let` are deprecated. `` | E8002, with a target |
| `DEFINE INDEX i ON t FIELDS a COUNT` | `Cannot create a count index with fields` | E1033 |

We used to refuse them too. For a language server that is the wrong trade: a
parse error is fatal to the whole source, so refusing one statement silences
the analyzer on the entire file, and all the user gets is a token-level
syntax error. The grammar now accepts them and the analyzer names the
contract at the span that is actually wrong — **E2020** ("KILL takes a
live-query uuid"), **E2021** ("SHOW SINCE takes a versionstamp or
datetime") and **E4030** ("INSERT's RELATION and IGNORE modifiers are in the
order the engine parses"), each quoting the engine's own error text in its
help so the user sees what SurrealDB will say. The 2026-09-13 round adds
**E4009** ("LIVE SELECT with unsupported clause") for every clause a live
query takes past `WHERE`/`FETCH`, and **E1033** for a `COUNT` index that
also names `FIELDS`.
`crates/workspace/tests/engine_refused_syntax.rs` pins that every one of them
fires, and that the correctly-spelled neighbour (`KILL u'…'`, `KILL $id`,
`SHOW … SINCE …`, `INSERT RELATION IGNORE …`, a live query with only
`WHERE`/`FETCH`, a `COUNT` index with no `FIELDS`) stays silent.

`PARALLEL` and the bare `$x = 1;` parameter assignment are the two entries in
that table whose diagnostic is *conditional*: both are 8002, a
version-compatibility code, and like every other check in
`analyzer/version.rs`, it is gated on a configured
`analysis.surrealdb_version`. With no target set, `PARALLEL` parses and
nothing is said — the same position `<future>`, `DEFINE SCOPE`/`TOKEN`,
`SEARCH ANALYZER` and the fuzzy operators have been in. Whether an unset
target (documented as "the latest") should fire every known removal is a
live question for all six constructs at once, not a `PARALLEL` question.

Uuid *shape* makes no difference: 3.2.3 rejects
`KILL "018e0f3a-1234-7abc-8def-0123456789ab"` exactly as it rejects
`KILL "not-a-uuid"`. Only a `u'…'` literal or a parameter is a live-query id.

This also makes our grammar a strict superset of upstream
`surrealql-tree-sitter`, which matters because upstream's consumers are
meant to be able to adopt this grammar without their corpus tests
regressing. Node names for the shared index-kind clauses (`CountClause`,
`FullTextClause`, `DiskAnnClause`, `DiskAnnDistClause`) match upstream's for
the same reason.

## `ORDER BY count`

Making `count` a legal field name in value position left an inconsistency:
the grammar parsed the field but not an `ORDER BY` over it, which is a false
error on ordinary SurrealQL — a table with a `count` column is nothing
unusual, and `SELECT field1, count() FROM t GROUP field1 ORDER BY count
DESC` is the natural way to read the aggregate back.

Every member of `_nonReservedIdent` was checked in order position against a
live 3.2.3 before anything was admitted, plus `rand`:

| spelling | engine | grammar before | now |
| --- | --- | --- | --- |
| `ORDER BY count` | accepted | rejected | **accepted** |
| `ORDER BY count.total` | accepted | rejected | **accepted** |
| `ORDER BY order` / `key` / `start` / `limit` / `group` | accepted | accepted | accepted |
| `ORDER BY rand` | **rejected** — ``Unexpected token `;`, expected (`` | rejected | rejected |

`rand` is genuinely reserved there: the engine takes only the `ORDER BY
RAND()` call form, so refusing a bare `rand` is correct strictness, and it
is pinned in the rejected corpus so a future loosening cannot swallow it by
accident. `order`/`key`/`start`/`limit`/`group` already worked — the
function-name keywords are the only ones `Idiom` cannot reach in this state,
because the clause's own `ORDER BY RAND()` alternative keeps them live as
tokens.

The fix is a hidden `_countIdiom` — an idiom rooted at `count`, aliased to
`Idiom` so the CST shape and every consumer are unchanged — offered beside
`$.Idiom` in `Order`. The idiom tail is factored into a hidden `_idiomTail`
shared by both, so the two cannot drift. `tree-sitter generate` reports no
conflict; no precedence annotation and no `conflicts` entry were needed. The
lowering test `lowers_order_by_count_as_a_field_not_a_call` pins that the
order key is the field path, not a call, and
`order_by_bare_rand_stays_a_parse_error` pins the other half.

## RATELIMIT was never SurrealQL

Three corpus entries claimed `DEFINE TABLE`/`DEFINE FIELD` take a
`RATELIMIT FOR <action> [WHERE …] [BY …] LIMIT n PER <duration> [MAX n]`
clause, and the grammar had a `RatelimitClause` to match. Nothing in
SurrealDB has ever had it.

Checked against surrealdb/surrealdb at rev `c7eac9022` (2026-08-28, newer
than the extraction that produced the entries):

- A pickaxe over **every ref** — `git log --all -i -G"ratelimit"` — returns
  four commits, all of them HTTP/WebSocket transport limiting under
  `server/` plus a `RateLimit` API error kind in
  `types/tests/error_types.rs`. No commit on any branch contains the token
  `RATELIMIT`.
- It is in no keyword table (`core/src/syn/lexer/keywords.rs`,
  `core/src/syn/token/keyword.rs`) at any tag from v1.0.0 to
  v3.3.0-beta.3.
- Live 3.2.3 does not lex it as a keyword at all:
  `DEFINE TABLE post RATELIMIT …` is ``Unexpected token `an identifier`,
  expected Eof`` underlining `RATELIMIT`, and `INFO FOR DB` has no bucket
  for it.

So it is not unreleased and not removed — 8003 and 8002 both have nothing
to name. The clause is gone from the grammar and the three entries moved to
the rejected set with that reason, which turns them from a claim the
grammar had to satisfy into one it must refuse. Valid 320 → 317, rejected
8 → 11.

## PARALLEL was real, and is gone

`ParallelClause` is wired into seven statements (SELECT through
`_modifierClause`, plus CREATE/UPDATE/UPSERT/DELETE/RELATE/INSERT). 3.2.3
refuses every one: ``Unexpected token `PARALLEL`, expected Eof``, verified
live on all seven.

Unlike RATELIMIT the clause existed. `sql/statements/{create,delete,insert,
relate,select,update,upsert}.rs` carry `parallel: bool` at every 1.x and
2.x tag, and from 2.0 the token parser eats it in
`syn/parser/stmt/<statement>.rs` — INSERT included, in both of 1.5.6's
parsers (`syn/v1/stmt/insert.rs:38`, `syn/v2/parser/stmt/insert.rs:76`) and
at `v2.3.7 crates/core/src/syn/parser/stmt/insert.rs:45`. It was deleted by 6d8302029, *"Remove
unused `PARALLEL` clause. (#6768)"*, which landed between `v3.0.0-beta.2`
(seven parser files still `self.eat(t!("PARALLEL"))`) and `v3.0.0-beta.3`
(none do) — so the removal ships in **3.0.0**, with no replacement: the
commit removed it because it did nothing.

That is what 8002 is for, so the clause keeps parsing and the version
registry names the removal. Deleting the rule would answer a 2.x user
migrating to 3.x with a token error that kills analysis of the whole file
and explains nothing. The six non-SELECT statements dropped the clause
during lowering and so had no span to report on; they now carry
`parallel: Option<ByteRange>` like `SelectStmt` does.

No corpus entry moved — neither set contained a `PARALLEL` statement. It is
pinned instead by `lowers_parallel_on_every_statement_that_took_it`
(crates/syntax) and `parallel_on_every_statement_that_took_it_is_8002`
(crates/workspace).

## INSERT takes RELATION before IGNORE

The grammar had `optional(IGNORE) optional(RELATION)` — the engine's order
backwards — so the only spelling SurrealDB accepts did not parse, and the
one it refuses did.

`syn/parser/stmt/insert.rs` reads
`let relation = self.eat(t!("RELATION")); let ignore = self.eat(t!("IGNORE"));`
and has since v2.0.5, where `RELATION` arrived; v1.5.6's `syn/v1/stmt/insert.rs`
has `IGNORE` and no `RELATION` at all. So the reversed order is not old
syntax for a version diagnostic to name — it is simply wrong, in every
release.

On 3.2.3, `INSERT RELATION IGNORE INTO likes {…}` inserts the edge and
`INSERT IGNORE RELATION INTO likes {…}` is ``Unexpected token `INTO`,
expected Eof`` — an error pointing at the word *after* the mistake, naming
neither keyword. The grammar takes both orders and **E4030** reports the
order, spanning both keywords. Valid corpus 317 → 319:
`INSERT RELATION IGNORE INTO likes {…}` and `INSERT RELATION INTO likes {…}`,
both run on 3.2.3, ratchet the order the grammar must keep accepting.

## 2026-09-13: LIVE SELECT's contract, and seven more engine-drift fixes

`THROW`-as-expression, the bracketed `SET` target, `PATCH`-as-any-expression
and `LIMIT`/`START`/`TIMEOUT`-as-expressions (each already in this file's
commit history) were the first half of this pass. The rest:

**1. `LIVE SELECT` grows the same clause set `SELECT` has, and 4009 owns the
contract.** `LiveSelectStatement` used to admit only `WHERE`/`FETCH` after
`FROM`; 3.2.3 refuses every one of `ORDER BY`, `GROUP`, `LIMIT`, `START`,
`SPLIT`, `OMIT`, `TIMEOUT`, `PARALLEL`, `EXPLAIN` and `FROM ONLY` *while
parsing* (``Unexpected token `ORDER`, expected Eof`` and so on, one per
clause, verified live), and a comma-separated `FROM` stops at the comma. The
grammar now parses all of them, `LiveSelectStmt` carries every field
`SelectStmt` does, and `LiveSelectStmt::as_select` converts one into the
other so `check_live_select` — which already existed for the `defineLive`
string path — judges both spellings through one function. 4009 now fires
for a real `LIVE SELECT`, not only a string literal passed to `defineLive`.

**2. Prefix `NOT` never existed.** The grammar's `PrefixExpression` listed
`NOT` beside `!`/`-`/`+` as a general prefix operator; 3.2.3 has no such
thing — only the `not(...)` builtin, which is a `FunctionCall`, not a
prefix. `RETURN NOT true;` is `` Unexpected token `true`, expected Eof ``
live; `RETURN NOT (true);` and `RETURN not(true);` both still parse, as the
same call. `NOT` is dropped from `PrefixExpression`'s operator choice
entirely — no replacement needed, since the call form already covered every
case that parses on the engine.

**3. A bare `$x = 1;` (no `LET`) is 1.x/2.x syntax, removed in 3.0.** 3.2.3:
`` Parameter declarations without `let` are deprecated. Replace with `let $x
= ...` `` — a hard parse error that kills the whole file, unlike a runtime
warning. The grammar still parses it as an ordinary equality expression
(`Statement::Expr(Binary { op: Eq, lhs: Param, .. })`), and
`version.rs::bare_param_assignment` fires 8002 when the statement *is*
exactly that shape and the target is 3.0+; `RETURN $x = 1;` and `WHERE $x =
1` are unaffected; they reach a different statement, not `Statement::Expr`.

**4. A record range never parses — in `FOR` or anywhere else.** `FOR $x IN
user:1..user:9 { … }` is `` Unexpected token `:`, expected { `` live, and
parentheses do not help. This is not FOR-specific: `RETURN user:1..user:9;`,
`SELECT * FROM user:1..user:9;` and `LET $r = user:1..user:9;` all fail the
same way, because `user:1..` is always the start of that record id's own
embedded range (`RecordIdRange`), which takes a plain id, never another
whole `tb:id`. `Range`'s left operand is narrowed (`_rangeStart`, a
`_baseValue` missing only `RecordId`) so a bare record id can no longer
start a *general* range; the right side is untouched (`RETURN 1..user:9;`
and `RETURN $a..user:9;` both run on 3.2.3), and `user:1..9` (the id's own
embedded range) is unaffected everywhere, including as a `FOR` iterable
(where it still fails at runtime — "Cannot execute statement using value" —
not at parse time, since a range value is not iterable, an existing 2022
concern).

**5. `FLEXIBLE` only ever follows `TYPE`.** `DEFINE FIELD f ON t FLEXIBLE
TYPE object;` — the clause order the published docs show — is `` Parse
error: FLEXIBLE must be specified after TYPE `` on 3.2.3; only `TYPE object
FLEXIBLE` runs. `TypeClause`'s `FLEXIBLE TYPE` alternative is removed.
Two of the analyzer's own fixtures wrote the wrong order (a corpus schema
file and two test schemas) and are fixed alongside the grammar.

**6. Every `fn::` parameter needs an explicit type.** `DEFINE FUNCTION
fn::greet($name) { … };` is `` Unexpected token ')', expected : `` live — a
closure's parameter (`|$v| $v`) stays untyped, but `DEFINE FUNCTION`'s own
list requires `: <kind>`. `_defineFunctionOptions` now takes
`_typedParamDefinition` (`VariableName Colon Type`, mandatory) instead of
the shared `ParamDefinition`, aliased to the same `ParamDefinition` node so
lowering needs no second shape.

**7. A `COUNT` index takes no `FIELDS`.** `DEFINE INDEX i ON t FIELDS a
COUNT;` is `Cannot create a count index with fields` on 3.2.3 — a
statement-level check in the engine's parser, not a context-free grammar
rule (`FIELDS` and the index-kind clause are independent repeated clauses
either could omit). The grammar still parses the combination and **E1033**
names it (broadening that code's existing `DEFINE FIELD` contract — "a
clause the definition accepts" — to `DEFINE INDEX` too, rather than minting
a new number for the same shape of mistake).

**8. `geometry<...>` is closed to seven names.** `geometry<pointt>` is
`` Unexpected token 'an identifier', expected a geometry kind name `` live.
`ParameterizedType` gets a geometry-specific alternative closed to `point`,
`line`, `polygon`, `multipoint`, `multiline`, `multipolygon` and
`collection` (plus a pipe-separated union of them — `geometry<point |
line>` also runs on 3.2.3); `_kw_geometry` outranks the generic identifier
by lexical precedence, so the closed form is always tried first and the
generic `ParameterizedType` branch never sees `geometry<...>` at all. This
closes a corresponding gap in `crates/workspace/src/schema.rs`'s
`geometry_kind` (which already modeled the same seven names at the
*semantic* level, for a shape the grammar had never restricted) — its
`None` fallback is now unreachable through anything that parses, and is
kept only as the belt to the grammar's suspenders.

Valid corpus 334 → 362 (28 new entries: the eleven LIVE SELECT clause
shapes, the bare parameter assignment, the correct `FLEXIBLE` order, a typed
`fn::` parameter, a `COUNT` index with `FIELDS`, the seven geometry kinds
plus one union, `NOT (…)`/`not(…)`, and the record-id-range shapes that must
keep parsing). Rejected corpus 11 → 18 (seven: bare-idiom `NOT` twice, the
record range in and out of `FOR`, the wrong `FLEXIBLE` order, an untyped
`fn::` parameter, and an unknown geometry kind).

### Where this grammar stands relative to the standalone `tree-sitter-surrealql`

A separate, from-scratch rewrite at `github.com/…/tree-sitter-surrealql`
(mirroring `@surrealdb/lezer` node names 1:1) was measured for a wholesale
swap during this round. Rule/node-kind names overlap almost completely
(233/233 of the standalone's named kinds already exist in the vendored
grammar's `node-types.json`; 563/571 `grammar.js` rules in common), but
dropping its `grammar.js`/`src/` into this crate and rebuilding broke 25 of
115 lowering/grammar tests and **105 of 334 (31%) of the then-current valid
corpus** — `KILL`, `NOT`, optional chaining, `?~`, `s'...'` strings,
`PARALLEL` on several statements, `THROW`/`IF` as expressions, MTREE
`DISTANCE`, `DEFINE EVENT ... ASYNC RETRY/MAXDEPTH`, `DEFINE ACCESS`/`DEFINE
USER`, and `LIMIT`/`TIMEOUT`/`PATCH` as expressions all regressed — several
of them work already landed on this branch. It also does not yet solve
item 1 above: its `LiveSelectStatement` only takes `WHERE`/`FETCH`, and its
`FROM` takes no `$param` source. The rejected corpus showed zero regressions
either way (over-acceptance did not increase), which is the one clean
result.

**Not viable as a wholesale adoption right now.** The lineages are close in
*shape* (hence the shared node-kind names above), but the standalone parser
is materially behind on everyday constructs and does not close this task's
own gap list. Where a standalone rule shape was usable as a reference (its
`TypeClause` FLEXIBLE-either-order handling, informing item 5 above) it was
read and adapted rather than invented from scratch, per the project's usual
practice of citing engine/upstream evidence rather than guessing. A future
migration would need `KILL`, `DEFINE ACCESS`/`USER` (+ `REMOVE`/`INFO`),
`LIVE SELECT`'s full clause set, `PARALLEL` on every statement that takes
it, `LIMIT`/`START`/`TIMEOUT`/`PATCH`/`THROW` as expressions, optional
chaining, string-literal prefixes, MTREE options, and event
`ASYNC`/`RETRY`/`MAXDEPTH` before it reaches current parity — before any new
work that would justify the switch at all.
