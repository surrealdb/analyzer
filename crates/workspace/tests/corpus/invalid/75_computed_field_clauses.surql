-- 1033: `COMPUTED` excludes every other clause that could also decide the
-- field's value or writability. Verified on 3.2.3, `OVERWRITE`d against a
-- fresh field so the answer is not masked by a 1022 redefinition.
DEFINE TABLE computed_probe SCHEMAFULL;

DEFINE FIELD OVERWRITE c ON computed_probe TYPE int COMPUTED 1 DEFAULT 2;

DEFINE FIELD OVERWRITE c ON computed_probe TYPE int COMPUTED 1 VALUE 2;

DEFINE FIELD OVERWRITE c ON computed_probe TYPE int COMPUTED 1 READONLY;

DEFINE FIELD OVERWRITE c ON computed_probe TYPE int COMPUTED 1 ASSERT $value > 0;

-- 1022 (via 1033's family): `in`/`out` are already fields of a relation table,
-- implicitly, before any DEFINE FIELD names them — "The field 'in' already
-- exists". A private relation table, so this fixture's (invalid) `in`
-- redefinition never leaks into another file's use of a shared one.
DEFINE TABLE computed_probe_edge SCHEMAFULL TYPE RELATION FROM computed_probe TO computed_probe;

DEFINE FIELD in ON computed_probe_edge TYPE record<computed_probe>;

-- 1033: a COMPUTED field is never stored, so indexing one has nothing to
-- index — "Computed fields cannot be indexed. Index: 'idouble' - Field:
-- 'double'".
DEFINE FIELD double ON computed_probe TYPE int COMPUTED 1 + 1;

DEFINE INDEX idouble ON computed_probe FIELDS double;
