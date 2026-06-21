// S339 — Array.prototype.with(Any idx, val) widen.
// ES §23.1.3.39 step 2 ToIntegerOrInfinity accepts arbitrary-typed
// input. Pre-S339 the method-table sig `(Number, T) -> Array<T>`
// strict-rejected `o: any` operands at typecheck. Pattern A
// (Any → Number → i64) sister to S331/S332/S333/S334/S335.
//
// Side-effect verification: step()-counter probe per Any-operand
// expression confirms eval-then-discard semantics fire once per
// site.

let calls = 0
function step<T>(v: T): T { calls++; return v }

// 1. any-from-binding — Number-typed Any
const xs: number[] = [10, 20, 30, 40, 50]
const o: any = 2
const r1 = xs.with(o, 99)
console.log(JSON.stringify(r1))

// 2. any-from-fn-return — Number returned as Any
function pickIdx(): any { return step(1) }
const r2 = xs.with(pickIdx(), 88)
console.log(JSON.stringify(r2))

// 3. number-literal fast path unchanged
const r3 = xs.with(0, 7)
console.log(JSON.stringify(r3))

// 4. NaN-Any — ToIntegerOrInfinity NaN → 0
const nanA: any = Number.NaN
const r4 = xs.with(nanA, 77)
console.log(JSON.stringify(r4))

// 5. negative Any idx — len + i wrap
const negA: any = -1
const r5 = xs.with(negA, 66)
console.log(JSON.stringify(r5))

// 6. step()-fired Any idx
const r6 = xs.with(step(3) as any, 55)
console.log(JSON.stringify(r6))

// 7. string-array shape
const ss: string[] = ["a", "b", "c"]
const sIdx: any = 1
const r7 = ss.with(sIdx, "Z")
console.log(JSON.stringify(r7))

// 8. boolean-Any → ToIntegerOrInfinity true → 1
const trueA: any = true
const r8 = xs.with(trueA, 44)
console.log(JSON.stringify(r8))

console.log("calls=" + calls)
