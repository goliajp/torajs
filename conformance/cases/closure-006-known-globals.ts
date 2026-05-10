// V3-18 m1.h.14 — closure capture set must not include known
// JS globals (NaN, Infinity, undefined, Object, Array, JSON,
// Math, Number, String, Boolean, Symbol, Date, RegExp, Error,
// Map, Set, Promise, BigInt, parseInt, parseFloat, isNaN,
// isFinite, encodeURI, decodeURI, globalThis, ...).
//
// Pre-fix: `is_global_name` only knew `console` and `Math`, so
// every other global referenced inside an arrow body got recorded
// as a "capture" and check.rs then bailed with
// `closure '__closure_N' references unknown identifier 'X'`.
// In test262 this single bug masked every annexB case that touched
// NaN / Function / Array.isArray / JSON.* etc inside arrows.
//
// Now: only true free vars (let-bound outer names) become captures;
// well-known globals resolve at the call site as before.

let isOdd = (n: number): boolean => n % 2 === 1
console.log(isOdd(3))
console.log(isOdd(4))

let arr = [1, 2, 3]
let isArr = (x: number): number => Array.isArray(arr) ? x + 100 : x
console.log(isArr(5))

let toJson = (x: number): string => JSON.stringify(x)
console.log(toJson(42))

let outer = 100
let combine = (x: number): number => outer + x + 1
console.log(combine(-3))
