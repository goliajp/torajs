// P11.5 — full Unicode default case fold (UCD 16.0) per ES
// §22.1.3.29 / §22.1.3.30. Pre-P11.5 tora only ASCII-folded; any
// non-ASCII letter passed through unchanged. P11.5-A1 adds Simple
// 1-cp -> 1-cp mappings from UnicodeData.txt; P11.5-A2 adds Full
// 1-cp -> N-cp expansions from SpecialCasing.txt Unconditional
// entries. Final_Sigma (`Σ -> ς` word-final) is a P11.5-A3
// follow-up (needs Cased property + context check); this fixture
// stays away from word-final Σ to keep its expectations stable.
//
// Coverage:
// - ASCII (no regression)
// - Latin-1 accented (no regression — already passed through correctly)
// - Greek / Cyrillic / Armenian (Simple — most letters round-trip)
// - Multi-script mixed
// - Full mapping: ß -> SS, fi-ligature -> FI, ffi-ligature -> FFI
// - Full mapping: İ -> i̇ (precomposed -> decomposed)
// - empty / single-char edge

console.log('--- ASCII regression ---')
console.log('ABC'.toLowerCase())
console.log('abc'.toUpperCase())
console.log(''.toUpperCase())
console.log(''.toLowerCase())

console.log('--- Latin-1 accented ---')
console.log('Café'.toLowerCase())
console.log('café'.toUpperCase())
console.log('Ñoño'.toUpperCase())
console.log('Größe'.toLowerCase())

console.log('--- Greek Simple ---')
console.log('ΑΒΓΔΕ'.toLowerCase())
console.log('αβγδε'.toUpperCase())
console.log('Ω'.toLowerCase())

console.log('--- Cyrillic ---')
console.log('Здравствуйте'.toUpperCase())
console.log('ПРИВЕТ'.toLowerCase())

console.log('--- Armenian ---')
console.log('Բարեւ'.toUpperCase())
console.log('ԲԱՐԵՒ'.toLowerCase())

console.log('--- Full mapping ß -> SS ---')
console.log('ß'.toUpperCase())
console.log('Straße'.toUpperCase())

console.log('--- Full mapping ligatures ---')
console.log('ﬁ'.toUpperCase())
console.log('ﬃ'.toUpperCase())
console.log('ﬄ'.toUpperCase())

console.log('--- Full mapping İ -> i + combining dot above ---')
console.log('İ'.toLowerCase())
console.log('İ'.toLowerCase().length)

console.log('--- Mixed scripts ---')
console.log('Hello, ΚΟΣΜΕ! Café? Ω.'.toLowerCase())

console.log('--- Round-trip ---')
console.log('αβγ'.toUpperCase().toLowerCase())
console.log('ΑΒΓ'.toLowerCase().toUpperCase())

console.log('--- Self-equality (Substr -> Str) ---')
console.log('Café'.slice(0, 4).toLowerCase() === 'café')
