// A flatMap callback that answers a non-array contributes its return
// as the element itself, per ES §23.1.3.11 step 8.d.
//
// The analysis spelled that case the same way as the array case, as
// the element OF the callback's return — which for a scalar names the
// element of a number, a class nothing else is in. So the product
// never joined the binding that received it, the two settled on
// different widths, and the read reinterpreted the bits.
//
// Both directions appear below: the callback wider than the binding
// started it, and the binding wider than the callback is the mirror
// image that the product's own cast site has to answer.

const xs: number[] = [1, 2];

// fraction comes from the callback
const a: number[] = xs.flatMap((x: number): number => x + 0.5);
console.log(a[0], a[1]);

// fraction is written into the product afterwards
const b: number[] = xs.flatMap((x: number): number => x * 2);
b[0] = 1.5;
console.log(b[0], b[1]);

// a named scalar callback, same shape
function half(x: number): number {
  return x + 0.25;
}
const c: number[] = xs.flatMap(half);
console.log(c[0], c[1]);

// an all-integral class stays narrow
const d: number[] = xs.flatMap((x: number): number => x * 3);
console.log(d[0], d[1]);

// string scalars are unaffected
const e: string[] = xs.flatMap((x: number): string => "s");
console.log(e[0], e[1]);

// the array-returning form still agrees with itself
const f: number[] = xs.flatMap((x: number): number[] => [x + 0.5]);
console.log(f[0], f[1]);
