// S340 — String.fromCodePoint(Any) widen.
// ES §22.1.2.2 step 2 ToNumber accepts arbitrary-typed input.
// RangeError throw (non-integer / out-of-range [0, 0x10FFFF]) is
// enforced by the runtime helper `str_from_code_point` via the
// pending-throw + emit_throw_check propagation, so the Any path
// inherits the same shape automatically. Pattern A sister to
// S329 (fromCharCode Any).

let calls = 0
function step<T>(v: T): T { calls++; return v }

// 1. any-from-binding — single-arg
const cpA: any = 65
console.log(String.fromCodePoint(cpA))

// 2. any-from-fn-return
function pick(): any { return step(0x1F600) }
console.log(String.fromCodePoint(pick()))

// 3. number-literal fast path unchanged
console.log(String.fromCodePoint(0x2764))

// 4. variadic Any — mixed Any/Number/Any
const cpB: any = 72
const cpC: any = 105
console.log(String.fromCodePoint(cpB, 32, cpC))

// 5. step()-fired Any cp
console.log(String.fromCodePoint(step(33) as any))

// 6. boolean-Any → ToNumber true → 1 → ""
const trueA: any = true
console.log(String.fromCodePoint(trueA).length)

// 7. supplementary-plane Any
const sup: any = 0x1F4A9
console.log(String.fromCodePoint(sup))

// 8. RangeError throw on out-of-range Any
try {
  const bad: any = 0x110000
  String.fromCodePoint(bad)
  console.log("NO THROW")
} catch (e) {
  console.log("THROW: " + (e instanceof RangeError ? "RangeError" : "?"))
}

// 9. RangeError throw on negative Any
try {
  const neg: any = -1
  String.fromCodePoint(neg)
  console.log("NO THROW")
} catch (e) {
  console.log("THROW: " + (e instanceof RangeError ? "RangeError" : "?"))
}

console.log("calls=" + calls)
