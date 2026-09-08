// A destructuring default compiles to a guard ternary over the
// source, so the two branches are the DEFAULT's object literal and
// whatever the source held. Those two rarely have the same shape, and
// the struct join demanded exactly that — equal field count, equal
// names in order — even though its answer is Any either way and its
// own note argues Any is correct for every field combination (each
// branch boxes by its own layout; the any-member read normalizes at
// runtime).
const { w: { x, y, z } = { x: 4, y: 5, z: 6 } } = { w: { x: undefined, z: 7 } };
console.log(x, y, z);

const { p: { a, b } = { a: 1, b: 2 } } = { p: { a: 9 } };
console.log(a, b);

const { q: { m } = { m: 3 } } = {};
console.log(m);

// a plain ternary over two differently-shaped literals answers
// through the any lanes
const cond = 1 < 2;
const o: any = cond ? { u: 1, v: 2 } : { u: "s" };
console.log(o.u, o.v);
const o2: any = cond ? { k: 1 } : { j: 2, l: 3 };
console.log(o2.k, o2.j);

// same-shape pairs keep answering as before
const o3: any = cond ? { n: 1 } : { n: 2 };
console.log(o3.n);
