// Phase 1c — P9.3-A1: unicode flag `u` mechanics.
// `\u{HHHH..}` extended escape; `\uHHHH` 4-digit escape (also fixes a
// pre-existing parser bug — `\uHHHH` now works in non-u patterns too);
// `.` consumes a single code point (1-4 bytes) under u flag via the
// Thread.u_skip outer-step defer queue.

// Extended `\u{}` — BMP code point.
console.log(/\u{0041}/u.test("A"))
console.log(/\u{0041}/u.test("B"))
console.log(/\u{03A9}/u.test("Ω"))

// Extended `\u{}` — astral code point (emoji).
console.log(/\u{1F600}/u.test("😀"))
console.log(/\u{1F600}/u.test("a"))
console.log(/\u{1F4A9}/u.test("💩"))

// `\uHHHH` 4-digit — works in both u and non-u modes (also fixes the
// pre-existing parser bug where `\uHHHH` parsed as literal `u<digits>`).
console.log(/A/.test("A"))
console.log(/A/.test("a"))
console.log(/A/u.test("A"))

// `.` astral under u flag — consumes one code point (4 bytes for emoji).
console.log(/^.$/u.test("😀"))
console.log(/^.$/u.test("a"))
console.log(/^.$/u.test("Ω"))
console.log(/^.$/u.test("ab"))

// `.match(/./u)` returns the full code-point string, not a half-byte.
{
  const m = "😀".match(/./u)
  console.log(m === null ? "null" : m[0])
}
{
  const m = "Ω".match(/./u)
  console.log(m === null ? "null" : m[0])
}
{
  const m = "a".match(/./u)
  console.log(m === null ? "null" : m[0])
}

// `.+` over multiple astral chars.
{
  const m = "😀😀😀".match(/.+/u)
  console.log(m === null ? "null" : m[0])
}

// Mixed ASCII + astral.
{
  const m = "a😀b".match(/./u)
  console.log(m === null ? "null" : m[0])
}
{
  const m = "a😀b".match(/.+/u)
  console.log(m === null ? "null" : m[0])
}

// Literal multi-byte char in pattern (Ω is U+03A9, 2 bytes UTF-8 0xCE 0xA9).
console.log(/Ω/u.test("Ω"))
console.log(/Ω/u.test("a"))

// Literal emoji in pattern (😀 is U+1F600, 4 bytes UTF-8).
console.log(/😀/u.test("😀"))
console.log(/😀/u.test("😢"))

// `\u{}` with leading zeros / variable hex digit count.
console.log(/\u{61}/u.test("a"))
console.log(/\u{0061}/u.test("a"))
console.log(/\u{00000061}/u.test("a"))

// Anchor + astral roundtrip.
{
  const m = "😀x".match(/^./u)
  console.log(m === null ? "null" : m[0])
}
{
  const m = "x😀".match(/.$/u)
  console.log(m === null ? "null" : m[0])
}
