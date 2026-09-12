import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// The generated golden (`test-d/gen/surrealql-analyzer.d.ts`) and the consumer
// beside it import the package by its published name, as real generated code
// does. tsc resolves that through the `paths` entry in tsconfig.json; vitest
// does not read tsconfig paths, so the same mapping is stated here for the
// runtime test.
export default defineConfig({
  resolve: {
    alias: { "@surrealdb/analyzer-client": fileURLToPath(new URL("./src/index.ts", import.meta.url)) },
  },
});
