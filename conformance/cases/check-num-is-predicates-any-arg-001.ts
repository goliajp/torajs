// S341 — Number.is{NaN,Finite,Integer,SafeInteger}(Any) silent-wrong fix.
//
// Pre-S341 the ssa_lower short-circuit at ~18056 dropped any non-{I64,F64,
// I32} operand to a static ConstBool(false), so an Any-boxed real Number
// (`const o: any = 2; Number.isInteger(o)`) returned false vs bun's true.
// The check.rs S202 carve-out at ~6860 had already widened the typecheck
// to accept any arg type (spec §21.1.2.{3,4,5,7} is strict-Type, and Any
// must dispatch on the unboxed tag).
//
// S341 routes the Any arg through the new `__torajs_num_is_*_any(tag, val)`
// runtime helpers — tag ∈ {ANY_I64=2, ANY_F64=3} dispatches the existing
// _i/_f impl; every other tag returns false per spec strict-Type-check.

let calls = 0
function step<T>(v: T): T { calls++; return v }

// 1. ANY_I64 — direct binding holds an int
const iA: any = 42
console.log(Number.isInteger(iA))
console.log(Number.isFinite(iA))
console.log(Number.isNaN(iA))
console.log(Number.isSafeInteger(iA))

// 2. ANY_F64 — direct binding holds a float
const fA: any = 1.5
console.log(Number.isInteger(fA))
console.log(Number.isFinite(fA))
console.log(Number.isNaN(fA))
console.log(Number.isSafeInteger(fA))

// 3. ANY_F64 — NaN / Infinity
const nanA: any = Number.NaN
console.log(Number.isNaN(nanA))
console.log(Number.isFinite(nanA))
console.log(Number.isInteger(nanA))
const infA: any = Number.POSITIVE_INFINITY
console.log(Number.isFinite(infA))
console.log(Number.isInteger(infA))

// 4. ANY_BOOL — non-Number tag → false
const bA: any = true
console.log(Number.isInteger(bA))
console.log(Number.isFinite(bA))
console.log(Number.isNaN(bA))
console.log(Number.isSafeInteger(bA))

// 5. ANY_HEAP — string → false
const sA: any = "5"
console.log(Number.isInteger(sA))
console.log(Number.isFinite(sA))
console.log(Number.isNaN(sA))
console.log(Number.isSafeInteger(sA))

// 6. ANY_NULL — null → false
const nA: any = null
console.log(Number.isInteger(nA))

// 7. Any from fn return (side-effect probe)
function getN(): any { return step(100) }
console.log(Number.isInteger(getN()))
console.log(Number.isSafeInteger(getN()))

// 8. Safe-integer boundary via Any
const maxA: any = 9007199254740991
console.log(Number.isSafeInteger(maxA))
const overA: any = 9007199254740992
console.log(Number.isSafeInteger(overA))

console.log("calls=" + calls)
