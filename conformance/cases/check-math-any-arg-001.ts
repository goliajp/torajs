// S342 — Math.<unary/binary>(Any[, Any]) widen + Any → F64 coerce.
// ES §21.3.2.* — every Math method applies ToNumber to its args;
// the method-table sigs `(Number) -> Number` / `(Number, Number) -> Number`
// strict-rejected `o: any` operands at typecheck. Pattern A sister to
// S327/S329/S331/S332/S333/S334/S335/S339/S340 — but the ssa_lower
// coerce now runs through any_to_number → F64 instead of the i64-coerce
// chain (Math helpers are f64-native).

let calls = 0
function step<T>(v: T): T { calls++; return v }

// 1. unary Math.{abs,floor,ceil,round,trunc,sign,sqrt}(Any-bound)
const a: any = 1.5
console.log(Math.abs(a))
console.log(Math.floor(a))
console.log(Math.ceil(a))
console.log(Math.round(a))
console.log(Math.trunc(a))
console.log(Math.sign(a))
console.log(Math.sqrt(a))

// 2. unary log family (use integer-aligned values to avoid f64-print drift)
const ten: any = 10
console.log(Math.log10(ten))
console.log(Math.cbrt(ten))

// 3. trig
const piA: any = Math.PI
console.log(Math.sin(piA).toFixed(15))
console.log(Math.cos(piA).toFixed(2))

// 4. Any from fn return — side-effect probe
function getN(): any { return step(-4) }
console.log(Math.abs(getN()))
console.log(Math.sqrt(Math.abs(getN())))

// 5. binary Math.pow(Any, Any)
const baseA: any = 2
const expA: any = 10
console.log(Math.pow(baseA, expA))
console.log(Math.atan2(baseA, expA).toFixed(5))

// 6. binary Math.{min,max}(Any, Any)
const xA: any = 3
const yA: any = 7
console.log(Math.min(xA, yA))
console.log(Math.max(xA, yA))

// 7. variadic Math.min/max(Any, Any, Any) ≥ 3 args
console.log(Math.min(xA, yA, step(1) as any, 5))
console.log(Math.max(xA, yA, step(99) as any, 5))

// 8. mixed Number + Any
console.log(Math.max(10, xA))
console.log(Math.min(10, xA, 5))

// 9. NaN-Any propagation
const nanA: any = Number.NaN
console.log(Math.abs(nanA))
console.log(Math.max(1, nanA))

// 10. boolean-Any → ToNumber true=1, false=0
const trueA: any = true
console.log(Math.abs(trueA))
console.log(Math.pow(trueA, 5))

console.log("calls=" + calls)
