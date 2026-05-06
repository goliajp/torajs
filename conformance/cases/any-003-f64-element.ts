// T-10.d.ii (v0.4.0) — F64 element in heterogeneous Array<Any>.
// Lowered through BitCastF64ToI64 SSA op to stash the IEEE 754 bit
// pattern in the ANY_F64 tagged slot's value field; print_any
// reverses the bitcast at decode time via memcpy.
let xs: any[] = [1.5, 'pi', 2.5, 42]
console.log(xs.length)
console.log(xs[0])
console.log(xs[1])
console.log(xs[2])
console.log(xs[3])

// Mixed F64 + I64 + Bool. The "1.5" / "0.25" floats coexist with
// integer-valued literals; tr's `number` defaults to i64 for
// integer literals, so 7 in this slot lowers as ANY_I64 not
// ANY_F64. Verifies the per-elem static type drives the tag.
let mixed: any[] = [1.5, true, 7, 0.25]
console.log(mixed[0])
console.log(mixed[1])
console.log(mixed[2])
console.log(mixed[3])

// Negative + scientific-shape floats — bit-pattern round-trip must
// preserve sign bit + exponent bits exactly.
let neg: any[] = [-3.14, 'x', -0.0]
console.log(neg[0])
console.log(neg[2])
