// P11.6-S5 — String.prototype.normalize("NFC") canonical composition
// per ES §22.1.3.13 + UAX #15 §3 D40. Decomposes (canonical only),
// canonical-orders by CCC, then primary-composes pairs back including
// Hangul L+V / LV+T algorithmic composition.
//
// Coverage:
// - Already-NFC input round trips (idempotent)
// - Decomposed input recomposes (e + ́ -> é)
// - Multi-char decomposed input recomposes character by character
// - Hangul jamo recomposes algorithmically (L+V -> LV, L+V+T -> LVT)
// - Compatibility forms stay NOT decomposed (ﬁ stays ﬁ under NFC)
// - ASCII passthrough unchanged
// - Empty string

console.log('--- already NFC ---')
console.log('café'.normalize('NFC').length)
console.log('café'.normalize('NFC') === 'café')
console.log('hello'.normalize('NFC') === 'hello')

console.log('--- decomposed -> composed ---')
// e + COMBINING ACUTE -> é
const dec_e = 'é'
console.log(dec_e.length, dec_e.normalize('NFC').length)
console.log(dec_e.normalize('NFC') === 'é')
// a + COMBINING GRAVE -> à
const dec_a = 'à'
console.log(dec_a.normalize('NFC') === 'à')
// Multi-decomposed -> composed token by token
const dec_word = 'café'
console.log(dec_word.length, dec_word.normalize('NFC').length)
console.log(dec_word.normalize('NFC') === 'café')

console.log('--- Hangul algorithmic compose ---')
// L + V -> LV (가 U+AC00)
const jamo_lv = '가'
console.log(jamo_lv.length, jamo_lv.normalize('NFC').length)
console.log(jamo_lv.normalize('NFC') === '가')
// L + V + T -> LVT (각 U+AC01)
const jamo_lvt = '각'
console.log(jamo_lvt.length, jamo_lvt.normalize('NFC').length)
console.log(jamo_lvt.normalize('NFC') === '각')

console.log('--- NFC ignores compat ---')
// ﬁ U+FB01 has only a <compat> decomposition -> NFC must leave it
console.log('ﬁ'.normalize('NFC') === 'ﬁ')
// µ U+00B5 has a compat -> μ -> NFC leaves it
console.log('µ'.normalize('NFC') === 'µ')

console.log('--- ASCII passthrough ---')
console.log('abc'.normalize('NFC') === 'abc')
console.log('1234'.normalize('NFC') === '1234')

console.log('--- empty ---')
console.log(''.normalize('NFC') === '')
console.log(''.normalize('NFC').length)
