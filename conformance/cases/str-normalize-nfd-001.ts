// P11.6-S5 — String.prototype.normalize("NFD") canonical
// decomposition per UAX #15 §3 D40 (canonical only, then canonical
// ordering by CCC). No composition pass.
//
// Coverage:
// - Precomposed letter expands (é -> e + ́; length 1 -> 2)
// - Multi-precomposed word expands (café -> 5 cu)
// - Hangul LV -> L + V (2 cu); LVT -> L + V + T (3 cu)
// - Already-decomposed input round trips
// - Compat-only entries STAY (NFD ignores `<tag>` mappings)
// - ASCII passthrough unchanged
// - Empty string

console.log('--- precomposed -> decomposed ---')
console.log('é'.length, 'é'.normalize('NFD').length)
console.log('é'.normalize('NFD') === 'é')

console.log('--- multi-letter word ---')
console.log('café'.length, 'café'.normalize('NFD').length)
console.log('café'.normalize('NFD') === 'café')

console.log('--- Hangul algorithmic decompose ---')
// 가 U+AC00 (LV) -> ᄀ + ᅡ (2 cu)
console.log('가'.length, '가'.normalize('NFD').length)
console.log('가'.normalize('NFD') === '가')
// 각 U+AC01 (LVT) -> ᄀ + ᅡ + ᆨ (3 cu)
console.log('각'.length, '각'.normalize('NFD').length)
console.log('각'.normalize('NFD') === '각')
// Multi-syllable: 한국 -> 한국 (6 cu)
console.log('한국'.length, '한국'.normalize('NFD').length)

console.log('--- already decomposed ---')
const dec = 'é' // e + ́
console.log(dec.length, dec.normalize('NFD').length)
console.log(dec.normalize('NFD') === dec)

console.log('--- NFD ignores compat ---')
// ﬁ U+FB01 has only `<compat>` -> NFD must NOT expand
console.log('ﬁ'.length, 'ﬁ'.normalize('NFD').length)
console.log('ﬁ'.normalize('NFD') === 'ﬁ')

console.log('--- ASCII passthrough ---')
console.log('hello'.normalize('NFD') === 'hello')
console.log('123'.normalize('NFD') === '123')

console.log('--- empty ---')
console.log(''.normalize('NFD') === '')
console.log(''.normalize('NFD').length)
