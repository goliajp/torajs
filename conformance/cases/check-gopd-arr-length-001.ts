// RFC C5a — Object.getOwnPropertyDescriptor(arr, "length") returns a
// real data descriptor per spec ES §10.4.2.4: `{value: arr.length,
// writable: true, enumerable: false, configurable: false}`. Pre-fix
// tora's gOPD walked dynobj entries only, so Array.length reported
// undefined.
//
// Note: only typed-element arrays (Arr<I64> / Arr<F64> / etc.) cover
// the new path here. `Arr<Any>` (`any[]`) + gOPD trips a pre-existing
// drop-time crash unrelated to RFC C5a — tracked separately as a
// substrate follow-up. Once that's fixed, an Arr<Any> case is added.

const xs = [1, 2, 3];
const d = Object.getOwnPropertyDescriptor(xs, "length");
console.log(d.value); // 3
console.log(d.writable); // true
console.log(d.enumerable); // false
console.log(d.configurable); // false

// 5-element typed array — exercises a larger len value through the
// helper's NaN-box pair (still fits in the I64 imm fast path).
const big = [10, 20, 30, 40, 50];
const d2 = Object.getOwnPropertyDescriptor(big, "length");
console.log(d2.value); // 5
console.log(d2.writable); // true

// String-element array — `Arr<Str>` still hits the typed `Type::Arr(_)`
// fast path; len read is the same offset.
const names = ["alice", "bob"];
const d3 = Object.getOwnPropertyDescriptor(names, "length");
console.log(d3.value); // 2
console.log(d3.configurable); // false
