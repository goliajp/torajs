// §12.9.4 StringLiteral (rotation 577). The refusals — a malformed
// `\x` / `\u`, a raw LF or CR in the body — are negative cases and
// live in test262. This fixture is the other side: the shapes next to
// those gates that are LEGAL and must keep working.
//
// `x` and `u` are EscapeCharacters, so a malformed spelling is a
// SyntaxError rather than a literal `x` / `u`. Every WELL-FORMED
// spelling still cooks.
console.log("\x41", "A", "\u{41}", "\u{1F639}".length);
// A braced escape takes any digit count whose value stays in range,
// leading zeros included; the surrogate halves keep their own
// spelling and rejoin.
console.log("\u{00000041}", "\u{10FFFF}".length, "😹".length);
// A lone surrogate is a well-formed escape — the string value is a
// sequence of code units, not of scalar values.
console.log("\uD800".length, "\uD800".charCodeAt(0));
// The body rejects a raw LF / CR, but `<LS>` and `<PS>` are re-admitted
// by the production as their own alternatives.
console.log("a b".length, "a b".length);
// A LineContinuation is a backslash plus a line terminator sequence and
// contributes nothing to the value.
console.log("a\
b");
// A NonEscapeCharacter passes through as itself — `q` is not in the
// EscapeCharacter set, so `\q` is still `q`.
console.log("\q", "\n".charCodeAt(0), "\0".charCodeAt(0));
// Both quote flavours read the same body production.
console.log('\x41A', '\q', 'a b'.length);
