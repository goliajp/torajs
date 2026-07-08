// RFC 20260708-typed-arr-oob-read chunk 2 — `number[]` (F64-width)
// OOB index read answers the undefined-NaN sentinel: a quiet NaN
// whose payload no arithmetic produces, materialized on the OOB
// branch of the typed index lane. Consumers gate STATICALLY
// (is_undef_f64_source: number[] index reads + their let aliases)
// and re-check the bits at runtime — arithmetic results are out of
// the gate, so an AArch64-propagated payload (`a[oob] + 1`) still
// reads as a plain NaN per JS semantics. Stations wired: typeof /
// console print (in-expr + top-stmt) / strict-eq undefined /
// nullish / box-to-any / any-literal pack / let contagion.
// Pre-fix every lane below read a garbage slot (`a[5]` printed 8,
// typeof said number, === undefined said false).
//
// I64/Bool-width and nested-heap elems have no undefined
// representation in the slot — their OOB reads throw a catchable
// RangeError (loud, chunk 3 decides the full undefined semantics).

const a: number[] = [1.5, 2.5];

// print — the sentinel walks to "undefined".
console.log(a[5]);                            // undefined

// typeof two-state.
console.log(typeof a[5]);                     // undefined

// strict-eq against undefined — bits compare.
console.log(a[5] === undefined);              // true
console.log(a[5] !== undefined);              // false

// arithmetic — NaN propagation, never "undefined".
console.log(a[5] + 1);                        // NaN

// nullish picks the default on the sentinel, lhs otherwise.
console.log(a[5] ?? 42);                      // 42
console.log(a[0] ?? 42);                      // 1.5

// let contagion — the alias carries the gate.
const e = a[5];
console.log(typeof e);                        // undefined

// box into the any world — ANY_UNDEF, not a NaN box.
const xs: any[] = [a[5]];
console.log(xs[0]);                           // undefined

// in-bounds reads stay direct.
console.log(a[0]);                            // 1.5
console.log(a[1]);                            // 2.5

// negative index — property miss.
console.log(a[-1]);                           // undefined

// dynamic OOB index.
const i: number = 9;
console.log(a[i]);                            // undefined
