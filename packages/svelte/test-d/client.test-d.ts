/**
 * `setClient` takes a `ClientCore`. See
 * `packages/query/test-d/client.test-d.ts` for why that matters: the
 * registry-parameterised client is not assignable to the defaulted one,
 * because `defineQuery`/`defineLive` differ in their return types and returns
 * are covariant.
 *
 * The first assertion is the mask-proof one — a program that augments the
 * global registry (this package's own `test-d/fixtures.ts` does) makes the two
 * instantiations relate again, so only "a bare `ClientCore` is enough" still
 * fails if this package goes back to demanding the full client.
 *
 * There is deliberately no `declare module` in this file. Pure type checks:
 * `adapters` is never called, so no context is ever set.
 */

import { createClient, type ClientCore } from "@surrealdb/analyzer-client";
import { dehydrate, hydrate, setClient, useClient } from "../src/index.js";

/** Stands in for the generated file; the shape is what `generate` emits. */
type Queries = {
  "SELECT id, name FROM person": {
    result: [Array<{ id: string; name: string }>];
    params: Record<string, never>;
  };
};

declare const core: ClientCore;

function adapters() {
  // Exactly what an adapter may demand, and no more.
  setClient(core);
  useClient(core);
  dehydrate(core);
  hydrate(core, {});

  // …and a client carrying its own registry is one of those.
  const db = createClient<Queries>({ url: "ws://localhost:8000/rpc" });
  setClient(db);
  useClient(db);
  dehydrate(db);
}
void adapters;
