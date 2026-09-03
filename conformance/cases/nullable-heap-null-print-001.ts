// A pointer-shaped `T | null` needs no box tax because the slot has
// a bit pattern to spare: `null` is the in-band 0. The console path
// already knew that slot could hold the generic undefined oddball
// and tested for it — and then handed everything else to the typed
// printer, which for an object, array or closure is `print_anyv`.
// `print_anyv` read the raw 0 as a NaN box and answered
// `[unknown-any-tag]`, so `console.log(q)` on a null object printed
// a diagnostic string instead of `null`. The oddball needed the same
// second test its undefined sibling already had.

type O = { x: number };

let q: O | null = null;
console.log(q);

// The value can arrive there by assignment too.
let r: O | null = { x: 1 };
console.log(r);
r = null;
console.log(r);

// A class instance is the same slot.
class C {
  a = 1;
}
let c: C | null = null;
console.log(c);
c = new C();
console.log(c.a);

// Arrays and closures share it.
let arr: number[] | null = null;
console.log(arr);
arr = [1, 2];
console.log(arr);

let fn: ((n: number) => number) | null = null;
console.log(fn);

// Through a parameter.
function viaParam(o: O | null): void {
  console.log(o);
}
viaParam(null);
viaParam({ x: 2 });

// Alongside other arguments, and inside a template of them.
const z: O | null = null;
console.log("v:", z, 1);
console.log(z, z);

// The undefined sibling still answers, and still answers first.
let u: O | undefined;
console.log(u);
