// RFC 20260708-spread-call chunk 1 — dynamic spread-call against a
// fixed-arity callee. `f(a0, …, ...src)` (src a typed-array Ident)
// desugars at the AST layer (`apply_spread_args`, after
// apply_rest_args) into index-read direct expansion:
//
//   f(a0, …, src[__torajs_spread_guard(src.length, need)],
//            src[1], …, src[need-1])
//
// The guard returns 0 (so the first arg reads slot 0) and throws
// when src.length < need — args evaluate left to right, so the
// guard runs before any element read; once it passes every
// constant index is in bounds, keeping the typed-array OOB lane
// unreached. Elements ride the existing Index-read lanes: typed
// slot direct read (zero boxing) for `T[]`, P1.4 box read for
// `any[]` into `any` params. Excess elements are ignored (JS
// semantics — a fixed-arity callee never observes them).
//
// Out of scope at this chunk (checker keeps the pre-existing loud
// rejects): `any[]` sources into *typed* params (needs the
// call-boundary Any admit + coerce pairing — chunk 2 with the
// `...arguments` face), Member/builtin callees, closure-value
// callees, non-Ident spread sources, non-trailing spreads.

function sum3(a: number, b: number, c: number): number { return a + b + c; }
function mix(tag: string, x: number, y: number): string { return tag + (x + y); }
function j2(a: string, b: string): string { return a + "|" + b; }
function anyPair(a: any, b: any): string { return "" + a + b; }

// number[] — typed slot direct expansion.
const arr: number[] = [10, 20, 12];
console.log(sum3(...arr));                    // 42

// fixed prefix args before the spread.
const xy: number[] = [7, 35];
console.log(mix("t", ...xy));                 // t42

// string[] — heap elements, borrow reads (no rc churn).
const ss: string[] = ["x", "y"];
console.log(j2(...ss));                       // x|y

// excess elements ignored.
const big: number[] = [1, 2, 3, 4, 5];
console.log(sum3(...big));                    // 6

// any[] into any params — Any→Any passes the existing admit.
const anyArr: any[] = [1, "z"];
console.log(anyPair(...anyArr));              // 1z
