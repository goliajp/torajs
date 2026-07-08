// RFC 20260708-spread-call chunk 2a — Any argument into a scalar /
// string parameter at the call boundary (TS any-assignability).
//
// Checker: the `any_into_scalar` admit (general.rs) accepts an Any
// arg against a Number / String / Boolean param, gated to plain
// Ident callees — `__cm_` class-method calls and Member-callee
// shapes route through dispatch lanes with no per-param coerce
// hook and stay loud, as do heap-typed params (no caller-side
// Any→heap unbox helper).
//
// SSA: every receiving lane pairs the admit with a coerce —
// terminal coerce_args / closure-local / fn-indirect: numbers via
// __torajs_anyv_to_number, bools via any_to_bool, strings via
// coerce_to_str(Any) minting a fresh-owned rc=1 Str released after
// the call.
//
// num_width: any-face slots (any[] elem, any params/rets) seed the
// F64 fixpoint so a number param fed from the any world carries
// ToNumber(undefined) = NaN faithfully — an I64 repr would
// FpToSi-truncate NaN to 0 (silent). This is what makes the
// undefined lanes below print NaN, matching bun.

function sum3(a: number, b: number, c: number): number { return a + b + c; }
function greet(name: string): string { return "hi " + name; }
function flag(b: boolean): string { return b ? "Y" : "N"; }
function fone(a: number): number { return a + 100.5; }

// Any → number param.
const av: any = 40;
console.log(sum3(av, 1, 1));                  // 42

// Any → string param (owned Str conversion, released post-call).
const asv: any = "bob";
console.log(greet(asv));                      // hi bob

// Any → boolean param.
const abv: any = 0;
console.log(flag(abv));                       // N

// any[] elem spread into number params (chunk 1 + 2a combined).
const anyArr: any[] = [10, 20, 12];
console.log(sum3(...anyArr));                 // 42

// undefined elem → ToNumber(undefined) = NaN, carried by the
// F64-seeded param repr.
const withUndef: any[] = [undefined, 1, 1];
console.log(sum3(...withUndef));              // NaN
console.log(fone(withUndef[0]));              // NaN

// closure callee (closure-local lane coerce).
const cb = (x: number): number => x * 2;
const ax: any = 21;
console.log(cb(ax));                          // 42
