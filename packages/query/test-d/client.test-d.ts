/**
 * The reactive core takes a `ClientCore`, and that is not a stylistic choice.
 *
 * `SurrealQLAnalyzerClient<Queries>` is NOT assignable to
 * `SurrealQLAnalyzerClient<GlobalRegistry>` — `defineQuery`/`defineLive` differ
 * in their RETURN types between the two, and returns are covariant however
 * bivariant methods are. Typed on the defaulted client, `getQueryClient(db)`
 * would reject every `createClient<Queries>` with an error naming a type the
 * caller never wrote.
 *
 * Two assertions, and the first is the one that cannot be masked: a program
 * that augments the global registry anywhere makes the two instantiations
 * relate again, so "a parameterised client is accepted" can pass for the wrong
 * reason. "A bare `ClientCore` is accepted" cannot — it fails the moment this
 * package asks for anything registry-shaped.
 *
 * There is deliberately no `declare module` in this file.
 */

import { createClient, type ClientCore } from "@surrealdb/analyzer-client";
import { getQueryClient, QueryClient } from "../src/index.js";

// Exactly what an adapter may demand, and no more.
declare const core: ClientCore;
getQueryClient(core);
new QueryClient(core);

// Stands in for the generated file; the shape is what `generate` emits.
type Queries = {
  "SELECT id, name FROM person": {
    result: [Array<{ id: string; name: string }>];
    params: Record<string, never>;
  };
};

const db = createClient<Queries>({ url: "ws://localhost:8000/rpc" });
getQueryClient(db);
new QueryClient(db);
