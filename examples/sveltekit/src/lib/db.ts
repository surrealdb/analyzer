// The client, in one place.
//
// `surrealkit generate` wrote `src/lib/surrealql-analyzer.d.ts` — an interface
// per table and a `Queries` type keyed by exact query text, and nothing that
// exists at runtime. The runtime is the package; the type argument joins them.

import { createClient } from "@surrealdb/analyzer-client";
import type { Queries } from "$lib/surrealql-analyzer";

// The one-line opt-in. `<Query q="SELECT …">` is a markup attribute with no
// call site to put a type argument on, so the components resolve their text
// through the GLOBAL registry — this merges the generated queries into it.
// It lives here, in the app's own code, rather than in the generated file:
// an augmentation counts only while its target resolves, and a generated one
// that silently stopped counting would leave every `<Query>` untyped with no
// error anywhere.
declare module "@surrealdb/analyzer-client" {
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
  interface SurqlRegistry extends Queries {}
}

export const RPC_URL = "ws://127.0.0.1:8124/rpc";

export const HEALTH_URL = "http://127.0.0.1:8124/health";

export const db = createClient<Queries>({
  url: RPC_URL,
  namespace: "demo",
  database: "demo",
  // `authentication`, not `signin`: the SDK reuses these credentials whenever a
  // session expires or the socket reconnects, so the demo survives a laptop
  // sleeping. A `.signin()` call — which the access-control beat makes — takes
  // precedence over this for the rest of that session.
  //
  // Root, because a root user BYPASSES table permissions: that is what makes
  // "signed out" mean "sees every ticket" in the last beat. A real app would
  // never ship this.
  authentication: { username: "root", password: "root" },
});
