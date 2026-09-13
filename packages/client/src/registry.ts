/**
 * The query registry — the contract between `surrealkit generate` and this
 * client.
 *
 * `surrealkit generate` emits a `.d.ts` containing one entry per analyzed
 * query, keyed by the exact query text:
 *
 * ```ts
 * // src/surrealql-analyzer.d.ts — generated; types only, nothing at runtime
 * export type Queries = {
 *   "SELECT * FROM user": {
 *     result: [Array<{ id: RecordId<"user">; name: string }>];
 *     params: Record<string, never>;
 *   };
 * };
 * ```
 *
 * The user hands that to the client as a type argument, which is the whole
 * wiring:
 *
 * ```ts
 * import { createClient } from "@surrealdb/analyzer-client";
 * import type { Queries } from "./surrealql-analyzer";
 *
 * export const db = createClient<Queries>({ url });
 * ```
 *
 * Every lookup is one conditional generic on a literal type parameter — there
 * is no permissive `string` overload anywhere on the path, because a literal
 * is also a `string` and a fallback overload would rescue every mis-call.
 *
 * # The global registry, and why it is opt-in
 *
 * {@link SurqlRegistry} is an empty interface a user may augment:
 *
 * ```ts
 * declare module "@surrealdb/analyzer-client" {
 *   interface SurqlRegistry extends Queries {}
 * }
 * ```
 *
 * That is what types `db.query("…")` on a client built without the type
 * argument, and the Svelte `<Query q="…">` markup form, which has no call
 * site to put one on. It is the user's line, in the user's module, on
 * purpose: the generated file used to carry it, and a `declare module` is an
 * augmentation only while its target resolves — when it did not, TypeScript
 * reported the failure inside the generated file, dropped every entry, and
 * left the user's queries silently `any`. Written by hand, a broken import is
 * an error where its author can see it.
 */

import type { Jsonify } from "surrealdb";

/**
 * The SurrealDB SDK's own value classes, re-exported so the generated file (and
 * user code) needs a single import. These are the types the SDK **actually
 * decodes** — see {@link Json} for the serialised projection.
 *
 * They are re-exported as values, not merely as types: constructing a parameter
 * (`new RecordId("team", "red")`) is the difference between a query matching
 * and silently returning nothing.
 */
export { Decimal, Duration, RecordId, Uuid } from "surrealdb";
export type { RecordIdValue } from "surrealdb";

/** Minimal GeoJSON shape, matching the codegen convention. */
export type GeoJSON = { type: string; coordinates: unknown };

/**
 * A value as it survives a serialisation boundary — SvelteKit's `load`, a React
 * Server Component's props, `JSON.stringify`. This is the SDK's own `Jsonify`,
 * so the mapping is theirs and not a guess: `RecordId<"team">` becomes
 * `` `team:${string}` ``, and `Date` / `Duration` / `Uuid` / `Decimal` become
 * `string`.
 *
 * The reactive layer (`@surrealdb/analyzer-query` and both framework adapters) is
 * `Json<T>` throughout, because a React Server Component boundary accepts only
 * plain values and offers no transport hook to widen that. `db.run` is not — it
 * hands back the SDK's real values.
 */
export type Json<T> = Jsonify<T>;

/**
 * One registered query's result and parameter types. `result` is the
 * per-statement response tuple — one element per statement in source order —
 * so it is always an array, even for a one-statement query.
 */
export interface SurqlQueryShape {
  result: unknown[];
  params: Record<string, unknown>;
}

/**
 * What a registry is: query text to {@link SurqlQueryShape}. This is the
 * constraint on every `Registry` type parameter in the package, and the
 * generated `Queries` satisfies it.
 *
 * Note the generated file declares `Queries` as a type ALIAS rather than an
 * interface, and it has to: TypeScript grants an implicit index signature to
 * object type literals and not to interfaces (an interface can be reopened),
 * so `interface Queries { … }` would not satisfy `Record<string, …>` and
 * `createClient<Queries>()` would not compile.
 */
export type SurqlRegistryShape = Record<string, SurqlQueryShape>;

/**
 * Every analyzed query, keyed by its exact text. Empty here, and empty unless
 * the user augments it themselves — see the module doc above.
 */
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface SurqlRegistry {}

/**
 * {@link SurqlRegistry} in a form a type parameter can hold, and the default
 * for every `Registry` parameter in the package — so code written before
 * `createClient<Queries>` existed keeps resolving through the global.
 *
 * The mapped type is not decoration. `SurqlRegistry` is an interface and an
 * interface never satisfies `Record<string, …>`; mapping over it produces an
 * object type, which does.
 */
export type GlobalRegistry = { [Text in keyof SurqlRegistry]: SurqlRegistry[Text] };

/** A query whose parameters are all supplied. Adapters accept only these. */
export type Bound = Record<string, never>;

/**
 * The per-statement response tuple a registered query resolves to, looked up
 * in `Registry` — the client's type argument, or the global registry when it
 * has none.
 */
export type ResultOf<
  Q extends string,
  Registry extends SurqlRegistryShape = GlobalRegistry,
> = Q extends keyof Registry ? Registry[Q]["result"] : unknown[];

/** The named parameters a registered query reads. */
export type ParamsOf<
  Q extends string,
  Registry extends SurqlRegistryShape = GlobalRegistry,
> = Q extends keyof Registry ? Registry[Q]["params"] : Record<string, unknown>;

/**
 * The parameters argument for a query: required exactly when the query reads
 * parameters, forbidden (an empty rest) when it reads none, and *optional* for
 * a query whose params are an open record — a non-registered query, where we
 * know nothing and must not demand an object the caller cannot construct.
 *
 * The open-record branch is load-bearing and easy to get wrong: an earlier
 * draft omitted it, and because `Record<string, unknown>` does not extend
 * `Record<string, never>`, every dynamic query then demanded required params.
 */
export type ParamsArg<P> = P extends Record<string, never>
  ? []
  : Record<string, unknown> extends P
    ? [params?: P]
    : [params: P];

/**
 * A readable type error, in the style of Supabase's `ParserError`. Intersecting
 * a branded object with a string *literal* puts the message itself into the
 * compiler's output, so a stale generated file reports what to do about it
 * rather than degrading to `unknown` and failing later, somewhere else.
 */
export type SurqlError<Message extends string> = {
  readonly __surqlError: true;
} & Message;
