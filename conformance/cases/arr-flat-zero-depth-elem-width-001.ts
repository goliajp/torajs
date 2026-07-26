// `flat(0)` flattens nothing (§23.1.3.13 — a depth of 0 copies the
// elements as they are), so its product stands where the receiver
// stands, the way `slice`'s does.
//
// The width analysis read the depth as always 1 and put the product's
// elements in the class of the receiver's INNER numbers. A nested read
// then landed one level too deep, in a class nobody is in — which
// defaults narrow — so the f64 bits of a fractional element came back
// as an integer.

const xs: number[][] = [[1, 2], [3]];
xs[0][1] = 1.5;
const f0: number[][] = xs.flat(0);
console.log(f0.length, f0[0][0], f0[0][1]);

// the outer element is still the same array
const g: number[][] = [[4, 5]];
g[0][0] = 0.5;
const g0: number[][] = g.flat(0);
console.log(g0[0][0], g0[0][1]);

// widened after the call — one class either way
const h: number[][] = [[6, 7]];
const h0: number[][] = h.flat(0);
h[0][1] = 2.5;
console.log(h0[0][0], h0[0][1]);

// a real flatten still flattens, and its elements stay numbers
const d: number[][] = [[8, 9], [10]];
d[0][0] = 3.5;
const d1: number[] = d.flat();
console.log(d1[0], d1[1], d1[2]);
const d2: number[] = d.flat(1);
console.log(d2[0], d2[1], d2[2]);

// an all-integral receiver stays narrow and unaffected
const n: number[][] = [[11, 12]];
const n0: number[][] = n.flat(0);
console.log(n0[0][0], n0[0][1]);

// slice, the shape flat(0) shares — already correct, kept as the pair
const s: number[][] = [[13, 14]];
s[0][1] = 4.5;
const s0: number[][] = s.slice(0);
console.log(s0[0][0], s0[0][1]);
