/**
 * The provider and the server helpers take a `ClientCore`. See
 * `packages/query/test-d/client.test-d.ts` for why: the registry-parameterised
 * client is not assignable to the defaulted one, because `defineQuery` and
 * `defineLive` differ in their return types and returns are covariant.
 *
 * The first assertion is the mask-proof one — `test-d/hooks.test-d.ts` in this
 * package augments the global registry, and a non-empty global makes the two
 * instantiations relate again.
 *
 * There is deliberately no `declare module` in this file.
 */

import { createClient, type ClientCore } from "@surrealdb/analyzer-client";
import { SurrealQLAnalyzerProvider } from "../src/index.js";
import { dehydrate, hydrate } from "../src/server.js";

/** Stands in for the generated file; the shape is what `generate` emits. */
type Queries = {
  "SELECT id, name FROM person": {
    result: [Array<{ id: string; name: string }>];
    params: Record<string, never>;
  };
};

declare const core: ClientCore;

function App() {
  // Exactly what an adapter may demand, and no more.
  dehydrate(core);
  hydrate(core, {});

  // …and a client carrying its own registry is one of those.
  const db = createClient<Queries>({ url: "ws://localhost:8000/rpc" });
  dehydrate(db);

  return (
    <SurrealQLAnalyzerProvider client={db}>
      <SurrealQLAnalyzerProvider client={core}>{null}</SurrealQLAnalyzerProvider>
    </SurrealQLAnalyzerProvider>
  );
}
void App;
