// `[...set]` array-literal spread of a Set source (ES iterator
// protocol; matches the Array.from(set) shape shipped earlier as
// S141). Spread routes through the shared `ssa_lower_arr_from_set`
// helper, producing `Array<Any>` slots that downstream Arr-merge
// can stitch alongside other elements.

const sNums = new Set([1, 2, 3, 4])
const aNums = [...sNums]
console.log('nums', aNums.length, aNums)

// Pure spread of a string Set (no literal siblings).
const sLetters = new Set(['a', 'b', 'c'])
const aLetters = [...sLetters]
console.log('letters', aLetters)

// Spread two sets back-to-back.
const sA = new Set([10, 20])
const sB = new Set([30, 40])
const both = [...sA, ...sB]
console.log('both', both)

// Empty Set — no iter steps, result is empty Arr<Any>.
const sEmpty = new Set<number>()
const aEmpty = [...sEmpty]
console.log('empty', aEmpty.length, aEmpty)

// Set with mixed-tag entries — Set stores Any-tagged entries so the
// spread preserves heterogeneity inline. Constructor takes Any[] so
// the parser only sees a homogeneous element type.
const sMix = new Set([1, 'two'])
const aMix = [...sMix]
console.log('mix', aMix.length, aMix)

// Round-trip parity with Array.from(set) on the same source.
const sRound = new Set([5, 6, 7])
console.log(
  'roundtrip',
  Array.from(sRound).join(',') === [...sRound].join(','),
)
