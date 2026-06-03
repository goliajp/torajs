// P11.4 — `for (const c of <str>)` yields code points per ES §22.1.5
// (String iterator).  Each iteration produces a fresh Substr view:
// BMP code units are 1-cu views; supplementary-plane code points
// combine the high+low surrogate pair into a single 2-cu view.
//
// Pre-P11.4 the source surfaced
//   `not yet supported: ssa-lower: for-of source type Str not yet supported`
// so any program using string iteration died at compile time.
//
// Coverage:
// - empty / Latin-1 ASCII (loop runs 0 / N times correctly)
// - BMP CJK / Hangul — each char yields a 1-cu Substr
// - supplementary emoji — high+low surrogate combine to 1 iter, c.length 2
// - mixed Latin-1 / BMP / supplementary in one string
// - codepoint-escape `\u{1F600}` matches direct emoji literal
// - body uses `c.length` + `c.charCodeAt(0)` to exercise per-iter Substr
// - `break` and `continue` behave as expected (continue recomputes adv)

console.log('--- empty ---')
for (const c of '') {
  console.log('NEVER')
}

console.log('--- ASCII ---')
for (const c of 'abc') {
  console.log(c, c.length)
}

console.log('--- CJK ---')
for (const c of '中文') {
  console.log(c, c.length)
}

console.log('--- Hangul ---')
for (const c of '한국어') {
  console.log(c, c.length)
}

console.log('--- emoji ---')
for (const c of '😀😀') {
  console.log(c, c.length)
}

console.log('--- mixed ---')
for (const c of 'a😀中b') {
  console.log(c, c.length, c.charCodeAt(0).toString(16))
}

console.log('--- codepoint escape ---')
for (const c of '\u{1F600}') {
  console.log(c, c.length)
}

console.log('--- break ---')
let n = 0
for (const c of 'abcdef') {
  if (n === 2) break
  console.log(c)
  n = n + 1
}
console.log('after break n=', n)

console.log('--- continue ---')
let m = 0
for (const c of 'a😀b😀c') {
  m = m + 1
  if (c.length === 2) {
    continue
  }
  console.log(c)
}
console.log('after continue m=', m)
