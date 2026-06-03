// P11.3-A1 — `s.codePointAt(i)` per ES §22.1.3.3 with surrogate
// pair combine. Distinguishes from `charCodeAt` (§22.1.3.2) which
// returns the lone UTF-16 code unit at index `i`.
//
// Pre-A1: codePointAt collapsed to charCodeAt, so
// `"a😀b".codePointAt(1)` returned the high surrogate `0xD83D`
// instead of the full codepoint `0x1F600` (😀).
//
// Coverage:
// - Latin-1 byte (sub-0xFF), BMP non-surrogate (CJK U+4E2D)
// - emoji direct literal at high-surrogate position -> full codepoint
// - emoji direct literal at low-surrogate position  -> low alone (per spec)
// - Substr.codePointAt mirrors the Str path
// - lone surrogate at end of string (no partner) -> lone high
// - multi-emoji adjacent (combine each pair independently)

const s = 'a😀b'
console.log(s.charCodeAt(0).toString(16))
console.log(s.charCodeAt(1).toString(16))
console.log(s.charCodeAt(2).toString(16))
console.log(s.charCodeAt(3).toString(16))
console.log(s.codePointAt(0).toString(16))
console.log(s.codePointAt(1).toString(16))
console.log(s.codePointAt(2).toString(16))
console.log(s.codePointAt(3).toString(16))

const t = '中文'
console.log(t.charCodeAt(0).toString(16))
console.log(t.codePointAt(0).toString(16))
console.log(t.charCodeAt(1).toString(16))
console.log(t.codePointAt(1).toString(16))

const sub = 'a😀b'.slice(1)
console.log(sub.charCodeAt(0).toString(16))
console.log(sub.codePointAt(0).toString(16))
console.log(sub.charCodeAt(2).toString(16))
console.log(sub.codePointAt(2).toString(16))

const pair = '😀😀'
console.log(pair.codePointAt(0).toString(16))
console.log(pair.codePointAt(2).toString(16))

const latin = 'A'
console.log(latin.charCodeAt(0).toString(16))
console.log(latin.codePointAt(0).toString(16))

// 0-arg form defaults pos to 0
console.log('B'.charCodeAt().toString(16))
console.log('😀'.codePointAt().toString(16))
