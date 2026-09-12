// The codegen golden, consumed end to end. `test-d/gen/surrealql-analyzer.d.ts`
// is real `surrealkit generate` output over the fixture workspace in
// `crates/codegen/tests/fixtures/typecheck` (pinned byte-for-byte by
// `cargo test -p surrealql-analyzer-codegen --test golden`). This file is compiled by
// `tsc` as part of `pnpm typecheck` — so the `expectTypeOf` lines below are
// checked, not merely executed — and run by vitest for the runtime half.
//
// Note what is NOT here any more: an import of the generated file for its
// values. It has none. The runtime comes from the package, the types come
// from the generated `.d.ts`, and `createClient<Queries>` is the join.
import { describe, expect, expectTypeOf, it } from "vitest";
import {
  createClient,
  Decimal,
  Duration,
  RecordId,
  Uuid,
  type ParamsOf,
  type QueryResultOf,
  type ResultOf,
} from "../src/index.js";
import type { Person, Queries, Tables } from "../test-d/gen/surrealql-analyzer.js";

const db = createClient<Queries>({ url: "ws://localhost:8000/rpc" });
const { defineQuery, defineLive } = db;

describe("the generated golden", () => {
  it("parameterises the client, which supplies the runtime", () => {
    expect(typeof db.query).toBe("function");
    expect(typeof db.run).toBe("function");
    // The value classes come from the package, not from the generated file:
    // a param is constructed with the SDK's own class, which is what renders
    // as a record link rather than a quoted string.
    expect(new RecordId("team", "red").toString()).toBe("team:red");
    expect(typeof Decimal).toBe("function");
    expect(typeof Duration).toBe("function");
    expect(typeof Uuid).toBe("function");
  });

  it("keys each fixture query by its exact text", () => {
    // Every literal here must be a key of `Queries`, or `defineQuery` rejects
    // it at compile time — that *is* the runtime shape of the registry.
    const roster = defineQuery("SELECT id, name, joined FROM person");
    expect(roster.text).toBe("SELECT id, name, joined FROM person");
    expect(roster.key).toBe("SELECT id, name, joined FROM person");
    expect(roster.isLive).toBe(false);

    const live = defineLive("SELECT id, name, status FROM person");
    expect(live.liveText).toBe("LIVE SELECT id, name, status FROM person");
    expect(live.key).toBe("SELECT id, name, status FROM person");

    const bound = defineQuery("SELECT name FROM person WHERE team = $team").with({
      team: new RecordId("team", "red"),
    });
    expect(bound.params).toEqual({ team: new RecordId("team", "red") });
  });

  it("survives destructuring, because the methods do not read `this`", () => {
    const { defineQuery: define } = db;
    expect(define("SELECT id, name, age FROM person").text).toBe(
      "SELECT id, name, age FROM person",
    );
  });
});

// Type-level assertions. Compiling this block is the test; vitest treats
// `expectTypeOf` as a no-op at runtime.

// The schema is generated, not only the queries: one interface per table, and
// a `Tables` map keyed by SurrealQL name.
expectTypeOf<Tables["person"]>().toEqualTypeOf<Person>();
expectTypeOf<Person["id"]>().toEqualTypeOf<RecordId<"person">>();
expectTypeOf<Person["settings"]>().toEqualTypeOf<{
  locale?: string;
  notify: boolean;
  timezone: string;
}>();
// An edge table carries the links the engine puts on every row.
expectTypeOf<Tables["knows"]["in"]>().toEqualTypeOf<RecordId<"person">>();

// Each SDK value class lands where the schema declares it, `option<T>` is an
// optional key inside a row, and a closed object field is a closed object.
expectTypeOf<
  Queries["SELECT settings, mentor, external_id, tenure FROM person WHERE id = $id"]["result"]
>().toEqualTypeOf<
  [
    Array<{
      external_id: Uuid;
      mentor?: RecordId<"person">;
      settings: { locale?: string; notify: boolean; timezone: string };
      tenure: Duration;
    }>,
  ]
>();
expectTypeOf<ResultOf<"SELECT name, budget FROM team", Queries>>().toEqualTypeOf<
  [Array<{ budget: Decimal; name: string }>]
>();

// A literal-union field constrains both the row and the param.
expectTypeOf<
  ResultOf<"SELECT name, status, tags FROM person WHERE status = $status", Queries>
>().toEqualTypeOf<
  [Array<{ name: string; status: "active" | "retired"; tags: Array<string> }>]
>();
expectTypeOf<
  ParamsOf<"SELECT name, status, tags FROM person WHERE status = $status", Queries>
>().toEqualTypeOf<{ status: "active" | "retired" }>();

// Two statements, two slots: the LET contributes `null`.
expectTypeOf<
  ResultOf<
    "LET $cutoff = time::now() - 1w; SELECT name, joined FROM person WHERE joined > $cutoff",
    Queries
  >
>().toEqualTypeOf<[null, Array<{ joined: Date; name: string }>]>();

// Graph traversal and the edge table's implicit `in`/`out` links.
expectTypeOf<
  ResultOf<"SELECT ->knows->person.name AS friends FROM person:jane", Queries>
>().toEqualTypeOf<[Array<{ friends: Array<string> }>]>();
expectTypeOf<ResultOf<"SELECT in, out, since, weight FROM knows", Queries>>().toEqualTypeOf<
  [Array<{ in: RecordId<"person">; out: RecordId<"person">; since: Date; weight: number }>]
>();

// A `fn::` call resolves to its declared return type.
expectTypeOf<ResultOf<"RETURN fn::greet($name)", Queries>>().toEqualTypeOf<[string]>();
expectTypeOf<ParamsOf<"RETURN fn::greet($name)", Queries>>().toEqualTypeOf<{ name: string }>();

// A text that is not in the registry degrades to the SDK's `unknown[]`, never
// to `any`.
expectTypeOf<QueryResultOf<"SELECT nope FROM nowhere", Queries>>().toEqualTypeOf<unknown[]>();

// …and the same lookups through the GLOBAL registry answer `unknown[]`, because
// nothing in this package augments it. That is the opt-in this design removed
// from the generated file: a project that wants `db.query("…")` typed on an
// un-parameterised client writes the augmentation itself.
expectTypeOf<ResultOf<"SELECT name, budget FROM team">>().toEqualTypeOf<unknown[]>();

// Pure type checks against the client — `main` is never called.
async function main() {
  const [teams] = await db.query("SELECT name, budget FROM team");
  expectTypeOf(teams).toEqualTypeOf<Array<{ budget: Decimal; name: string }>>();

  // @ts-expect-error the text reads $status, so the params object is required.
  await db.query("SELECT name, status, tags FROM person WHERE status = $status");
  await db.query("SELECT name, status, tags FROM person WHERE status = $status", {
    // @ts-expect-error "hired" is outside the field's literal union.
    status: "hired",
  });
}
void main;
