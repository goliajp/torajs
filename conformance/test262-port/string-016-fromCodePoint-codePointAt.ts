// Adapted from test262: built-ins/String/fromCodePoint/* and
// String.prototype/codePointAt/* — Unicode-aware siblings of
// fromCharCode / charCodeAt. tr's byte-Str layout collapses the two
// for code points ≤ 0xff (the only range we exercise here so bun and
// tr agree byte-for-byte). fromCodePoint is variadic and shares the
// fromCharCode lowering chain; codePointAt aliases charCodeAt.
function check(): number {
  // fromCodePoint — single arg, ASCII.
  if (String.fromCodePoint(65) !== "A") { throw "#1: A"; }
  if (String.fromCodePoint(122) !== "z") { throw "#2: z"; }
  if (String.fromCodePoint(48) !== "0") { throw "#3: 0"; }

  // fromCodePoint — variadic.
  if (String.fromCodePoint(72, 105) !== "Hi") { throw "#4: Hi"; }
  if (String.fromCodePoint(97, 98, 99) !== "abc") { throw "#5: abc"; }

  // Empty + single matches fromCharCode parity.
  if (String.fromCodePoint() !== "") { throw "#6: empty"; }

  // Latin-1 byte (≤ 0xff) — tr stores bytes verbatim; bun emits the
  // same UTF-8 byte for codepoints < 128. Stay in ASCII to avoid
  // multi-byte UTF-8 encoding divergence.
  if (String.fromCodePoint(126) !== "~") { throw "#7: tilde"; }

  // Roundtrip with codePointAt.
  let s = "abc";
  if (s.codePointAt(0) !== 97) { throw "#8: codePointAt 0"; }
  if (s.codePointAt(1) !== 98) { throw "#9"; }
  if (s.codePointAt(2) !== 99) { throw "#10"; }

  // codePointAt agrees with charCodeAt in the ASCII range.
  let h = "Hello";
  if (h.codePointAt(0) !== h.charCodeAt(0)) { throw "#11: H"; }
  if (h.codePointAt(4) !== h.charCodeAt(4)) { throw "#12: o"; }

  // Roundtrip across both APIs.
  let code = "Q".codePointAt(0);
  if (String.fromCodePoint(code) !== "Q") { throw "#13: roundtrip"; }

  return 0;
}
console.log(check());
