// P11.6-S5 — String.prototype.normalize("NFKD") compatibility
// decomposition per UAX #15 §3 D42. Like NFD but also expands
// `<tag>` mappings (compat ligatures, super/sub, fractions, Roman
// numerals, ...). No composition pass.
//
// Coverage:
// - Compat ligature expands (ﬁ -> f + i; length 1 -> 2)
// - Compat micro sign maps to Greek (µ -> μ; same length, different cp)
// - Compat superscript / fraction decompose
// - Canonical decomposition still fires (é -> e + ́)
// - Hangul algorithmic still fires (NFKD applies it the same as NFD)
// - ASCII passthrough unchanged

console.log('--- compat ligatures ---')
console.log('ﬁ'.length, 'ﬁ'.normalize('NFKD').length)
console.log('ﬁ'.normalize('NFKD') === 'fi')
console.log('ﬃ'.length, 'ﬃ'.normalize('NFKD').length)
console.log('ﬃ'.normalize('NFKD') === 'ffi')

console.log('--- compat micro sign ---')
// µ U+00B5 -> μ U+03BC (length still 1)
console.log('µ'.length, 'µ'.normalize('NFKD').length)
console.log('µ'.normalize('NFKD') === 'μ')

console.log('--- compat superscript / fraction ---')
console.log('²'.length, '²'.normalize('NFKD').length)
console.log('²'.normalize('NFKD') === '2')
// ½ U+00BD -> 1 + FRACTION SLASH + 2 (length 3)
console.log('½'.length, '½'.normalize('NFKD').length)

console.log('--- canonical decomp still applies under NFKD ---')
console.log('é'.length, 'é'.normalize('NFKD').length)
console.log('é'.normalize('NFKD') === 'é')

console.log('--- Hangul still algorithmic ---')
console.log('가'.length, '가'.normalize('NFKD').length)
console.log('각'.length, '각'.normalize('NFKD').length)

console.log('--- ASCII passthrough ---')
console.log('hello'.normalize('NFKD') === 'hello')

console.log('--- empty ---')
console.log(''.normalize('NFKD') === '')
console.log(''.normalize('NFKD').length)
