// A host file saved with CRLF line endings — the shape that used to emit a
// generated file that does not parse. The template literal below spans two
// lines, so extraction sees `\r\n` where the RUNTIME sees `\n`: cooking a
// template normalises every line terminator to LF. The key has to be the
// cooked spelling, or it matches nothing at run time and its `\r` ends the
// emitted string literal early (TS1002, inside the generated file).
//
// This file is fixture input for the Rust golden test, never compiled itself.
// Keep its CRLF endings: `.gitattributes` beside it stops git normalising
// them, and without them it is testing nothing.

export const wrapped = () =>
  db.query(`SELECT name, age
FROM person
WHERE age > 21`);
