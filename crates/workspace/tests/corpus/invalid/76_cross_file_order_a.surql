-- Setup for 76_cross_file_order_b.surql (sorts after this file): a DEFINE
-- FIELD on a not-yet-defined table auto-creates it schemaless, so the
-- DEFINE TABLE there fails as a redefinition — "The table 'order_probe'
-- already exists". Split across two files on purpose (the bug is
-- cross-file, unlike a same-file DEFINE FIELD-then-TABLE, which already
-- reports on the field itself as 1001). This statement is not itself wrong,
-- so this file also carries its own, unrelated violation — every fixture
-- under invalid/ must report on its own.
DEFINE FIELD label ON order_probe TYPE string;

-- 1001: an unrelated reference to a table nothing ever defines.
SELECT * FROM ghost_order_reference;
