// P11.6-S5 — String.prototype.normalize("NFKC") compatibility
// decomposition + canonical composition per UAX #15 §3 D43. Expands
// `<tag>` mappings (compat ligatures, super/sub digits, compat
// fractions, Roman numerals, ...) and then composes canonical pairs
// back to precomposed cps.
//
// Coverage:
// - Compat ligature decomposes (ﬁ -> f + i; no composite)
// - Compat micro sign maps to Greek letter (µ -> μ)
// - Compat superscript decomposes (² -> 2)
// - Compat Roman numeral decomposes (Ⅳ -> IV)
// - Canonical compose still fires (decomposed é input -> precomposed)
// - ASCII passthrough unchanged
// - Empty string

console.log('--- compat ligatures ---')
console.log('ﬁ'.normalize('NFKC') === 'fi')
console.log('ﬁ'.normalize('NFKC').length)
console.log('ﬃ'.normalize('NFKC') === 'ffi')
console.log('ﬃ'.normalize('NFKC').length)

console.log('--- compat micro sign -> Greek mu ---')
// µ U+00B5 MICRO SIGN -> μ U+03BC GREEK SMALL LETTER MU
console.log('µ'.normalize('NFKC') === 'μ')
console.log('µ'.normalize('NFKC').length)

console.log('--- compat superscript / subscript ---')
// ² U+00B2 SUPERSCRIPT TWO -> 2
console.log('²'.normalize('NFKC') === '2')
// ½ U+00BD VULGAR FRACTION ONE HALF -> 1 + FRACTION SLASH + 2
console.log('½'.normalize('NFKC').length)

console.log('--- compat Roman numerals ---')
// Ⅳ U+2163 ROMAN NUMERAL FOUR -> IV
console.log('Ⅳ'.normalize('NFKC') === 'IV')

console.log('--- canonical compose still fires ---')
// Decomposed é input: NFKC decomposes (no compat for é decomp) then
// composes canonical pair back -> precomposed é.
const dec = 'é'
console.log(dec.length, dec.normalize('NFKC').length)
console.log(dec.normalize('NFKC') === 'é')

console.log('--- ASCII passthrough ---')
console.log('abc'.normalize('NFKC') === 'abc')

console.log('--- empty ---')
console.log(''.normalize('NFKC') === '')
console.log(''.normalize('NFKC').length)
