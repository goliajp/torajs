// S171 — `String.prototype.replaceAll("", repl)` per ES §22.1.3.20:
// empty needle inserts `repl` at every position (between each code
// unit + at both boundaries). Pre-S171 the runtime short-circuited
// to a fresh copy of `s` (silent stub note in the doc), so any
// program using `replaceAll("", X)` silently diverged from bun.

// 1) Pre-S171 wedge — interleave at every position.
console.log("aaa".replaceAll("", "X"));

// 2) "abc" interleave with single-char repl.
console.log("abc".replaceAll("", "*"));

// 3) Single-char s — just two repls around the char.
console.log("a".replaceAll("", "X"));

// 4) Empty s — single repl insertion.
console.log("".replaceAll("", "X"));

// 5) Empty repl + empty needle → no-op (regression guard).
console.log("abc".replaceAll("", ""));

// 6) Multi-char repl — verifies stride/output length math.
console.log("ab".replaceAll("", "yo"));

// 7) Mixed encoding — UTF-16 repl into latin-1 s upgrades encoding.
// `"∞"` is a 3-byte UTF-8 / 1 code unit BMP character; bun emits
// the interleaved result.
console.log("ab".replaceAll("", "∞"));

// 8) Regression — non-empty needle path unchanged.
console.log("aaa".replaceAll("a", "X"));
console.log("hello".replaceAll("l", ""));
console.log("xy".replaceAll("z", "Z"));
