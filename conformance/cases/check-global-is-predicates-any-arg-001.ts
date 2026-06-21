// S343 — global `isNaN(Any)` / `isFinite(Any)` silent-wrong fix.
// ES §19.2.{3,4} step 1: ToNumber accepts arbitrary-typed input —
// these globals coerce (intentional contrast with the strict
// `Number.is*` namespaced methods). Pre-S343 the ssa_lower coerce
// match (~17516) fell into the refcounted catch-all branch for
// Any args and returned `ConstBool(name == "isNaN")` — silent-
// wrong vs bun when the Any boxed a real Number (`const a: any =
// 42; isNaN(a)` → bun: false, tr: true). Distinct from the S341
// silent-wrong fix on the strict-Type `Number.is*` family.

let calls = 0
function step<T>(v: T): T { calls++; return v }

// 1. ANY_I64 — boxed integer
const iA: any = 42
console.log(isNaN(iA))
console.log(isFinite(iA))

// 2. ANY_F64 — boxed float
const fA: any = 1.5
console.log(isNaN(fA))
console.log(isFinite(fA))

// 3. ANY_F64 — NaN / Infinity
const nanA: any = Number.NaN
console.log(isNaN(nanA))
console.log(isFinite(nanA))
const infA: any = Number.POSITIVE_INFINITY
console.log(isNaN(infA))
console.log(isFinite(infA))

// 4. ANY_HEAP — string coerces via ToNumber
const sNum: any = "42"
console.log(isNaN(sNum))
console.log(isFinite(sNum))
const sBad: any = "not-a-number"
console.log(isNaN(sBad))
console.log(isFinite(sBad))

// 5. ANY_BOOL — ToNumber true=1, false=0
const tA: any = true
const fAB: any = false
console.log(isNaN(tA))
console.log(isFinite(tA))
console.log(isNaN(fAB))
console.log(isFinite(fAB))

// 6. ANY_NULL — ToNumber(null) = 0
const nA: any = null
console.log(isNaN(nA))
console.log(isFinite(nA))

// 7. Any from fn return — side-effect probe
function getN(): any { return step(100) }
console.log(isFinite(getN()))
console.log(isNaN(getN()))

// 8. trailing-arg drop semantics still fire side-effects
console.log(isFinite(iA, step(99) as any))

console.log("calls=" + calls)
