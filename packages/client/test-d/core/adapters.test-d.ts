// A parameterised client is accepted by everything that takes a client —
// **with no module augmentation anywhere in this file**.
//
// That last clause is the whole test. `SurrealQLAnalyzerClient<Queries>` is NOT
// assignable to `SurrealQLAnalyzerClient<GlobalRegistry>`: `defineQuery` and
// `defineLive` differ in their RETURN types between the two instantiations
// (`DefinedQuery<Q, Queries>` vs `DefinedQuery<Q, GlobalRegistry>`), and a
// return position is covariant however bivariant the method is. So an adapter
// typed on the defaulted client rejects every `createClient<Queries>` with an
// error naming a type the caller never wrote.
//
// `ClientCore` is the fix: the session plus everything keyed by a query VALUE,
// with no registry in any signature. `preload` and every adapter take that.
//
// It also has to be compiled in a program with NO augmentation in it at all,
// which is why this directory has its own `tsconfig.json`: an augmentation is
// program-wide, and a non-empty global registry makes the two instantiations
// relate again — hiding the exact bug this pins. `test-d/query.test-d.ts` (the
// opt-in path) is one, so the main program cannot host this test.
//
// Pure type checks — `main` is never called, so no connection opens.
import { createClient, preload, type ClientCore } from "@surrealdb/analyzer-client";
import type { Queries } from "../gen/surrealql-analyzer.js";
import type { Equal, Expect } from "../assert.js";

const db = createClient<Queries>({ url: "ws://localhost:8000/rpc" });
const roster = db.defineQuery("SELECT id, name, joined FROM person");
const livePeople = db.defineLive("SELECT id, name, status FROM person");

// The relation an adapter relies on, stated directly.
type _isCore = Expect<Equal<typeof db extends ClientCore ? true : false, true>>;

// Stands in for `setClient`, `getQueryClient`, `SurrealQLAnalyzerProvider`, and
// every hook that takes `{ client }` — all of them are this signature.
declare function takesClient(client: ClientCore): void;

async function main() {
  takesClient(db);
  takesClient(createClient({ url: "ws://localhost:8000/rpc" })); // and the un-parameterised one

  // `preload` is the real one, and the payload keeps its row type across the
  // serialisation boundary even though the adapter never saw `Queries`.
  const seeded = await preload(db, roster);
  type _seeded = Expect<
    Equal<(typeof seeded)["data"][number], { id: `person:${string}`; joined: string; name: string }>
  >;
  const seededLive = await preload(db, livePeople);
  type _seededLive = Expect<
    Equal<
      (typeof seededLive)["data"][number],
      { id: `person:${string}`; name: string; status: "active" | "retired" }
    >
  >;

  // The registry-free escape an adapter needs for a text it captured at
  // runtime: `unknown[]`, never `any`.
  const dynamic = await db.queryUnchecked("SELECT " + "1");
  type _dynamic = Expect<Equal<typeof dynamic, unknown[]>>;
}
void main;
