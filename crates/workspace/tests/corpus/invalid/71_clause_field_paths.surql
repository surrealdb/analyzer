-- The row-context clauses — OMIT, SPLIT, FETCH, GROUP BY, ORDER BY — read a
-- field path exactly as a projection and a condition do.
--
-- They were the last callers of the non-crossing resolver, which treats a
-- `record<>` field as an opaque boundary: it stopped at `owner` and never
-- asked whether the table behind it declares the rest. So every mistake past
-- a link was either silent here, or reported in the vocabulary of the wrong
-- contract ("SPLIT needs a collection field, but `owner.ghost` is a `none`" —
-- describing the absence as a kind problem).
--
-- `organization.owner` is a `record<account>`, and `account` has no `ghost`.

-- OMIT was silent outright: omitting a field that isn't there is a no-op the
-- author did not intend.
SELECT * OMIT owner.ghost FROM organization;

-- SPLIT and FETCH each reported their own kind contract against the vacuous
-- `none` an absent path resolves to. Now they report the absence itself, and
-- their kind findings (1024 / 1023) are reserved for a field that EXISTS and
-- is the wrong shape.
SELECT * FROM organization SPLIT owner.ghost;

SELECT * FROM organization FETCH owner.ghost;

-- GROUP BY and ORDER BY over an EXPLICIT projection list are the selection's
-- contract, not the schema's: SurrealDB 3.x does not run either query and
-- does not finish parsing it ("Missing group idiom `owner.ghost` in statement
-- selection" / "Missing order idiom ..."), and the fix — project the key — is
-- the same whether the key is a typo or a real field the list dropped. So
-- these are 4013 and 2017, and the help names the absent field.
SELECT name FROM organization GROUP BY owner.ghost;

SELECT name FROM organization ORDER BY owner.ghost;

-- A wildcard projection is the case the engine itself accepts: it takes any
-- ORDER BY idiom, sorts every row by NONE, and returns them in an order the
-- author did not ask for. Nothing rejects this but us — and because `*` hands
-- the whole source row through, the key really must be a field of it: 1002.
SELECT * FROM organization ORDER BY owner.ghost;

-- Multi-hop, to show the finding lands on the table actually missing the
-- field: `organization.owner` is a `record<account>` whose `person` is a
-- `record<person>`, and `person` has no `ghost`.
SELECT * FROM organization ORDER BY owner.person.ghost;

-- A link naming SEVERAL tables is wrong only when NO arm declares the field —
-- the projection's rule, now the clauses' rule too. `document.owner` is a
-- `record<account | organization>`; neither declares `ghost`.
SELECT * FROM document ORDER BY owner.ghost;

-- 2017: an ORDER BY key that names an ALIAS the query never projects. The
-- table has no `total` either, but the defect is the projection list, so the
-- reader is sent there rather than to the schema.
SELECT name, count() AS n FROM organization GROUP BY name ORDER BY total;
