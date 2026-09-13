// The client, in one place. Three lines, and they are the whole wiring:
//
//   1. the runtime comes from the package,
//   2. the types come from the generated file (which has no runtime at all),
//   3. the type argument joins them.
//
// `surrealkit generate` scanned this project, found every query text in it,
// analyzed each against `schema/schema.surql`, and wrote
// `src/surrealql-analyzer.d.ts`: an interface per table and a `Queries` type
// keyed by exact query text. Nothing in that file exists at runtime, so
// nothing imports it for a value.

import { createClient } from "@surrealdb/analyzer-client";
import type { Queries } from "./surrealql-analyzer";

// The connection opens lazily on first use, so a module-level client is safe
// and nothing has to remember to `await db.connect(...)`.
export const db = createClient<Queries>({
  url: "ws://localhost:8000/rpc",
  namespace: "demo",
  database: "demo",
});

// Bound to `Queries`, so a named query needs no module augmentation and no
// second import. Destructuring is safe: neither reads `this`.
export const { defineQuery, defineLive } = db;
