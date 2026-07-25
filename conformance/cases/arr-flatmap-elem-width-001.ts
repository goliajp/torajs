// The elements flatMap pushes must reach arr_push as raw bits.
//
// arr_push takes the slot as an i64, so handing it an f64 element as a
// value converted the number on the way in: 0.5 arrived as 0. Where the
// product's own layout was f64, the read afterwards turned that stored
// integer back into a denormal, so 1.5 came out as 5e-324 — silently,
// exit 0. Every other push site already bitcasts through the shared
// shorthand; this one built its own call and skipped it.
//
// Both directions of the same mistake are below: an element that is
// fractional only inside the callback, and one that is fractional
// before it ever gets there.

const xs: number[] = [1, 2];

// fraction produced inside the callback
const a: number[] = xs.flatMap((x: number): number[] => [x + 0.5]);
console.log(a[0], a[1]);

// fraction in a literal that ignores the parameter
const b: number[] = xs.flatMap((x: number): number[] => [0.5]);
console.log(b[0], b[1]);

// fraction in an array built outside the callback
const lit: number[] = [0.5, 1.5];
const c: number[] = xs.flatMap((x: number): number[] => lit);
console.log(c[0], c[1], c[2], c[3]);

// a named callback, same shape
function pair(x: number): number[] {
  return [x + 0.25, x];
}
const d: number[] = xs.flatMap(pair);
console.log(d[0], d[1], d[2], d[3]);

// an all-integral class stays narrow and unaffected
const e: number[] = xs.flatMap((x: number): number[] => [x, x * 2]);
console.log(e[0], e[1], e[2], e[3]);

// string elements are unaffected
const g: string[] = xs.flatMap((x: number): string[] => ["a", "b"]);
console.log(g[0], g[1], g[2], g[3]);

// a fractional value written into the product afterwards
const h: number[] = xs.flatMap((x: number): number[] => [x]);
h[0] = 2.5;
console.log(h[0], h[1]);
