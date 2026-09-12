// Proves the generated `.d.ts` feeds the whole client end to end — the three
// lines a user writes, and nothing else:
//
//   import { createClient } from "@surrealdb/analyzer-client";
//   import type { Queries } from "./surrealql-analyzer";
//   const db = createClient<Queries>({ url });
//
// No module augmentation anywhere in this file. That is the point: the
// generated file declares types and the client takes them as an argument, so
// a broken import fails at the `createClient<Queries>` line rather than
// silently dropping every query's types.
//
// Pure type checks — `main` is never called, so no connection opens.
import { createClient, RecordId } from "@surrealdb/analyzer-client";
import type { Person, Queries, Tables, Team } from "./surrealql-analyzer.js";
import type { Equal, Expect } from "../assert.js";

const db = createClient<Queries>({ url: "ws://localhost:8000/rpc" });
const { defineQuery, defineLive } = db;
const peopleOf = defineQuery("SELECT name FROM person WHERE team = $team");
const roster = defineQuery("SELECT id, name, joined FROM person");

// The schema is generated too, and a table interface is a type a user can put
// in their own signature — which is the other half of what `generate` is for.
declare function greet(person: Person): string;
type _person = Expect<Equal<Person["id"], RecordId<"person">>>;
type _optional = Expect<Equal<Person["nick"], string | undefined>>;
type _decimal = Expect<Equal<Team["budget"], Tables["team"]["budget"]>>;
void greet;

async function main() {
  // THE HEADLINE CLAIM, asserted exactly: a plain `db.query("…")` — no
  // `defineQuery`, no ceremony — resolves the real per-statement tuple from
  // the type argument alone.
  const direct = await db.query("SELECT id, name, joined FROM person");
  type _direct = Expect<
    Equal<typeof direct, [Array<{ id: RecordId<"person">; joined: Date; name: string }>]>
  >;
  direct[0][0]!.name.length;
  direct[0][0]!.joined.getTime();
  // @ts-expect-error `nope` is not on the row — a miss is an error, not `any`.
  direct[0][0]!.nope;

  // Params too: required exactly when the text reads them, and typed.
  const [directRed] = await db.query("SELECT name FROM person WHERE team = $team", {
    team: new RecordId("team", "red"),
  });
  directRed[0]!.name.length;
  // @ts-expect-error the text reads $team, so the params object is required.
  await db.query("SELECT name FROM person WHERE team = $team");
  // @ts-expect-error a string is not a RecordId<"team">.
  await db.query("SELECT name FROM person WHERE team = $team", { team: "team:red" });

  // Resolved from the generated entry through `db.defineQuery` — params
  // required and typed as the SDK class, which is what makes the query match
  // on the wire.
  const rows = await db.run(peopleOf, { team: new RecordId("team", "red") });
  rows[0]!.name.length;

  // Values are the SDK's, so a datetime really is a Date and a record link
  // really is a RecordId.
  const people = await db.run(roster);
  type _row = Expect<
    Equal<(typeof people)[number], { id: RecordId<"person">; joined: Date; name: string }>
  >;
  people[0]!.joined.getTime();
  people[0]!.id.table;

  // …and `Json<T>` is what survives an SSR boundary.
  const asJson = await db.runJson(roster);
  type _json = Expect<
    Equal<(typeof asJson)[number], { id: `person:${string}`; joined: string; name: string }>
  >;
  asJson[0]!.id.startsWith("person:");

  // A live query reads the same registry, and carries its row type.
  const livePeople = defineLive("SELECT id, name, status FROM person");
  const stop = db.watch(livePeople, (live) => {
    type _live = Expect<
      Equal<
        (typeof live)[number],
        { id: RecordId<"person">; name: string; status: "active" | "retired" }
      >
    >;
    live[0]!.name.toUpperCase();
  });
  stop();

  // @ts-expect-error the generated entry requires the params object
  await db.run(peopleOf);
  // @ts-expect-error a string is not a RecordId — the encode-time bug, caught
  await db.run(peopleOf, { team: "team:red" });
  // @ts-expect-error field not in the generated result shape
  (await db.run(peopleOf, { team: new RecordId("team", "red") }))[0]!.age;
  // @ts-expect-error a RecordId is not a string: no string methods on a record link
  people[0]!.id.startsWith("person:");

  // A text the generated file does not contain is a hard error carrying its
  // own remedy, not a silent degrade.
  const stale = defineQuery("SELECT nope FROM nowhere");
  // @ts-expect-error the generated registry has no such query
  await db.run(stale);
  void stale;
}
void main;
