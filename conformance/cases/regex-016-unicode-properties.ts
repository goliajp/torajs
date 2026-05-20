// Phase 1c — P9.3-A2: Unicode property classes (\p{L} / \p{N} / \p{ASCII})
// under u flag. ASCII portion lives in the bitmap; cp >= 128 portion is
// covered by curated UCD subset tables (Greek, Cyrillic, Hebrew, Arabic,
// CJK, Hangul, Hiragana, Katakana, common decimal-digit scripts).
//
// \P{X} outside class = class-level negate.
// \p{X} inside [...] = OR-union into the current class.
// Full UCD coverage + \P{X} inside class (complement) are L3b.

// \p{Letter} / \p{L} positive.
console.log(/\p{Letter}/u.test("a"))
console.log(/\p{Letter}/u.test("Z"))
console.log(/\p{L}/u.test("Ω"))
console.log(/\p{L}/u.test("漢"))
console.log(/\p{L}/u.test("я"))
console.log(/\p{L}/u.test("7"))
console.log(/\p{L}/u.test(" "))

// \p{Number} / \p{N}.
console.log(/\p{Number}/u.test("7"))
console.log(/\p{N}/u.test("0"))
console.log(/\p{N}/u.test("７"))
console.log(/\p{N}/u.test("a"))

// \p{ASCII}.
console.log(/\p{ASCII}/u.test("a"))
console.log(/\p{ASCII}/u.test("7"))
console.log(/\p{ASCII}/u.test(" "))
console.log(/\p{ASCII}/u.test("Ω"))
console.log(/\p{ASCII}/u.test("😀"))

// \P{X} — class-level negate.
console.log(/\P{L}/u.test("7"))
console.log(/\P{L}/u.test("a"))
console.log(/\P{N}/u.test("a"))
console.log(/\P{ASCII}/u.test("Ω"))
console.log(/\P{ASCII}/u.test("a"))

// match() with \p{L}+ — sequence of letters.
{
  const m = "Hello, 世界!".match(/\p{L}+/u)
  console.log(m === null ? "null" : m[0])
}
{
  const m = "abc123".match(/\p{L}+/u)
  console.log(m === null ? "null" : m[0])
}

// /\p{L}+/gu — global, multiple word-like clumps.
console.log("abc 漢字 def".match(/\p{L}+/gu))
console.log("漢字 abc Ω".match(/\p{L}+/gu))

// Union in class: [\p{L}\p{N}].
console.log(/[\p{L}\p{N}]/u.test("a"))
console.log(/[\p{L}\p{N}]/u.test("7"))
console.log(/[\p{L}\p{N}]/u.test("漢"))
console.log(/[\p{L}\p{N}]/u.test(" "))
console.log(/[\p{L}\p{N}]/u.test("!"))

// Class-level negate over property: [^\p{L}].
console.log(/[^\p{L}]/u.test("a"))
console.log(/[^\p{L}]/u.test("7"))
console.log(/[^\p{L}]/u.test(" "))
console.log(/[^\p{L}]/u.test("漢"))

// Mixed bitmap + property union: [a-z\p{N}].
console.log(/[a-z\p{N}]/u.test("a"))
console.log(/[a-z\p{N}]/u.test("7"))
console.log(/[a-z\p{N}]/u.test("Ω"))
console.log(/[a-z\p{N}]/u.test("Z"))

// \p{L}+ with replace.
console.log("Ω漢a1Ω".replace(/\p{L}+/gu, "X"))

// Anchored property in non-trivial pattern.
console.log(/^\p{L}+$/u.test("hello"))
console.log(/^\p{L}+$/u.test("hello1"))
console.log(/^\p{L}+$/u.test("漢字漢"))

// Property alias resolution.
console.log(/\p{Letter}/u.test("漢"))
console.log(/\p{L}/u.test("漢"))
console.log(/\p{Number}/u.test("７"))
console.log(/\p{N}/u.test("７"))
