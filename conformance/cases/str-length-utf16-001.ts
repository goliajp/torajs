// P11.2 — `String.length` returns the UTF-16 code unit count per
// ES §22.1.4.1, not byte size of any encoding. Pre-P11.1 tora's
// Str layout stored `length` as a byte count, which diverged from
// bun on every non-ASCII fixture. S1 flipped the Str layout
// (`length@8` = u32 code unit count + canonical encoding alloc),
// and S5 mirrored Substr. This fixture is the spec-parity gate
// for the full multi-encoding `.length` surface.
//
// Coverage:
// - empty / single Latin-1 / multi-Latin-1 ASCII
// - BMP non-ASCII single + multi (CJK + Hangul)
// - supplementary plane (emoji) single + multi via surrogate pair
// - mixed Latin-1 + BMP UTF-16 + supplementary
// - combining mark: decomposed (e + U+0301) vs precomposed (U+00E9)
// - lone surrogate halves (well-formed UTF-16 not enforced)
// - codepoint-escape literal `\u{1F600}` matches direct emoji literal

console.log(''.length)
console.log('a'.length)
console.log('hello'.length)

console.log('中'.length)
console.log('中文'.length)
console.log('한'.length)
console.log('한국어'.length)

console.log('😀'.length)
console.log('😀😀'.length)
console.log('😀😀😀'.length)

console.log('a中'.length)
console.log('a😀'.length)
console.log('a😀中'.length)
console.log('hello 中文 😀'.length)

console.log('é'.length)
console.log('é'.length)

console.log('\uD83D'.length)
console.log('\uDC00'.length)
console.log('😀'.length)

console.log('\u{1F600}'.length)
console.log(String.fromCodePoint(0x1f600).length)
console.log(String.fromCharCode(0xd83d, 0xde00).length)
