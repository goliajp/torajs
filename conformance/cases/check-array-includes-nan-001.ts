// ES §23.1.3.16 — `Array.prototype.includes(needle)` uses
// SameValueZero, which equates NaN with NaN. Pre-fix the F64 elem
// arm in tora's shared indexOf / lastIndexOf / includes loop used
// `fcmp oeq`, which is IEEE 754-ordered and rejects NaN-vs-NaN.
// `indexOf` / `lastIndexOf` correctly keep strict equality (NaN
// never matches).

// F64 receiver — NaN handling
const ns: number[] = [1.5, 2.5, NaN, 3.5]
console.log('ns.includes(NaN)', ns.includes(NaN))
console.log('ns.includes(2.5)', ns.includes(2.5))
console.log('ns.includes(9.0)', ns.includes(9.0))

// indexOf / lastIndexOf — NaN never matches (StrictEq, spec'd).
console.log('ns.indexOf(NaN)', ns.indexOf(NaN))
console.log('ns.lastIndexOf(NaN)', ns.lastIndexOf(NaN))
console.log('ns.indexOf(2.5)', ns.indexOf(2.5))

// +0 / -0 — SameValueZero treats them equal; IEEE 754 fcmp oeq
// already collapses them, so all three methods report a hit.
const zs: number[] = [0, 1, 2]
console.log('zs.includes(-0)', zs.includes(-0))
console.log('zs.indexOf(-0)', zs.indexOf(-0))

// I64 receiver — NaN never appears in an i64 slot; needle NaN
// short-circuits to false / -1 (existing behaviour, regression
// guard).
const is: number[] = [1, 2, 3]
console.log('is.includes(NaN)', is.includes(NaN))
console.log('is.indexOf(NaN)', is.indexOf(NaN))

// from-index argument still respected — includes from second
// position skips the leading match.
const dup: number[] = [NaN, 1.0, NaN]
console.log('dup.includes(NaN, 1)', dup.includes(NaN, 1))
console.log('dup.includes(NaN, 3)', dup.includes(NaN, 3))
