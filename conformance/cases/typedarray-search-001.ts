// §23.2.3 slab A search half — indexOf / lastIndexOf / includes.
//
// The point of the case is the two equalities and the one input
// they disagree on. `indexOf` is IsStrictlyEqual, under which NaN
// matches nothing; `includes` is SameValueZero, under which NaN
// matches NaN. Both treat +0 and -0 as the same value, so NaN is
// the whole of the difference.
//
// `lastIndexOf` additionally distinguishes an ABSENT fromIndex
// (start at len - 1) from an explicit `undefined` (coerces to 0, so
// it looks at index 0 alone).

const a = new Uint8Array([10, 20, 30, 20, 10]);

// indexOf — found, duplicate picks the first, missing, fromIndex
console.log(a.indexOf(20), a.indexOf(10), a.indexOf(99));
console.log(a.indexOf(20, 2), a.indexOf(20, 4), a.indexOf(10, -1));
console.log(a.indexOf(10, -99), a.indexOf(10, 99), a.indexOf(10, Infinity));
console.log(a.indexOf(10, -Infinity), a.indexOf(10, NaN), a.indexOf(10, 1.9));

// lastIndexOf — absent vs explicit undefined is the interesting pair
console.log(a.lastIndexOf(20), a.lastIndexOf(10), a.lastIndexOf(99));
console.log(a.lastIndexOf(10, undefined), a.lastIndexOf(20, undefined));
console.log(a.lastIndexOf(20, 2), a.lastIndexOf(20, 0), a.lastIndexOf(10, -1));
console.log(a.lastIndexOf(10, -99), a.lastIndexOf(10, 99));
console.log(a.lastIndexOf(10, -Infinity), a.lastIndexOf(10, Infinity));

// includes — same shape, boolean answer
console.log(a.includes(20), a.includes(99), a.includes(20, 2), a.includes(20, 4));
console.log(a.includes(10, -1), a.includes(10, -99), a.includes(10, Infinity));

// the one input the two equalities disagree on
const f = new Float64Array([1, NaN, 3]);
console.log(f.indexOf(NaN), f.includes(NaN));
console.log(f.indexOf(1), f.includes(1), f.indexOf(3), f.includes(3));

// +0 and -0 are the same value for both
const z = new Float64Array([0, -0]);
console.log(z.indexOf(-0), z.indexOf(0), z.includes(-0), z.includes(0));
console.log(1 / z[0], 1 / z[1]);

// a needle no element of this type can be
console.log(a.indexOf("20" as any), a.includes("20" as any));
console.log(a.indexOf(null as any), a.includes(null as any));
console.log(a.indexOf(undefined as any), a.includes(undefined as any));
console.log(a.indexOf(20.5), a.includes(20.5));

// empty views answer without looking
console.log(new Uint8Array(0).indexOf(0), new Uint8Array(0).includes(0));
console.log(new Uint8Array(0).lastIndexOf(0));

// float32 rounds on the way in, so the needle has to match what was
// STORED, not what was written
const g = new Float32Array([0.1, 0.5]);
console.log(g.indexOf(0.1), g.indexOf(0.5), g.includes(0.5));

// BigInt elements take a BigInt needle, and a Number is never one
const b = new BigInt64Array([1n, -2n, 3n]);
console.log(b.indexOf(-2n), b.lastIndexOf(3n), b.includes(1n));
console.log(b.indexOf(2 as any), b.includes(3 as any));

// A needle outside the element type's range is NOT probed here, and
// the omission is deliberate. bun answers `0` for
// `new BigUint64Array([0n, 1n]).indexOf(2n ** 64n)` — it truncates
// the needle to 64 bits before comparing — while contradicting
// itself one line earlier, since `2n ** 64n === 0n` is false in the
// same engine. node answers -1, and §23.2.3.14 step 8.b is
// IsStrictlyEqual, which for two BigInts is mathematical equality.
// tr follows the spec, so a probe of it could not be byte-equal to
// the oracle. Recorded rather than matched; the round-trip check
// that produces the spec answer is in `typedarray_search::needle`.
//
// The in-range half of the same question is safe to probe, and it
// is the one that catches a raw-bits comparison: -1n and
// 2n**64n-1n are the same 64 bits and different BigInts.
const sbuf = new ArrayBuffer(8);
new BigInt64Array(sbuf)[0] = -1n;
console.log(new BigInt64Array(sbuf).indexOf(-1n));
console.log(new BigUint64Array(sbuf).indexOf(18446744073709551615n));
