-- Deliberately unknowable shapes. A third-party webhook payload has no schema
-- the author can write down, so `any` here is CORRECT, not a precision loss.
-- The `any` ratchet needs at least one such site so its "expected, with a
-- reason" mechanism is exercised rather than merely documented.

DEFINE TABLE webhook_event SCHEMAFULL;

DEFINE FIELD provider ON webhook_event TYPE 'stripe' | 'github';

DEFINE FIELD received_at ON webhook_event TYPE datetime DEFAULT time::now();

-- Opaque by construction: the provider owns this shape.
DEFINE FIELD payload ON webhook_event TYPE any;

-- Shapeless but at least known to be an object. `FLEXIBLE` only ever follows
-- `TYPE object` on 3.2.3 — `FLEXIBLE TYPE object` is the clause order the
-- published docs show, and the engine refuses it: `Parse error: FLEXIBLE
-- must be specified after TYPE`.
DEFINE FIELD headers ON webhook_event TYPE object FLEXIBLE;

-- A table whose rows an external system owns, so the workspace declares no
-- fields on it. `SELECT * FROM external_record` gives up here, and so must a
-- `.*` on a link to it — same wall, same code (7008). The link is what makes
-- that reachable from a query.
DEFINE TABLE external_record SCHEMALESS;

DEFINE FIELD external ON webhook_event TYPE record<external_record>;
