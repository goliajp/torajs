// annexB §B.1.4 — a malformed `\xHH` escape (missing digits, non-
// hex byte after `\x`) falls back to IdentityEscape('x') outside
// u/v mode. Pre-fix: parse_hex_escape set the parser err flag and
// the whole regex compile failed.
// test262 target: annexB/built-ins/RegExp/incomplete_hex_unicode_escape.js.

// Bare `\x` — matches "x".
console.log(/\x/.test("x"));
// Only one hex digit `\xa` — matches "xa" (the 'a' reparses as
// literal 'a' after `\x` falls back to literal 'x').
console.log(/\xa/.test("xa"));
// Two hex digits still work as before (spec canonical form).
console.log(/\x41/.test("A"));
// Followed by non-hex non-word: `\x!` → "x!".
console.log(/\x!/.test("x!"));

// Followed by a hex digit but then a non-hex — `\x1` alone rewinds
// so the `1` reparses as a literal digit: pattern is "x1".
console.log(/\x1/.test("x1"));
