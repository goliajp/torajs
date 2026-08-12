// W4 shape-join — same-shaped object literals share one struct layout,
// so a later literal's fractional / NaN field must not truncate through
// the first registrant's I64 slot (rotation 371; surfaced as a set-like
// `size: NaN` reading back 0).
const a = { x: 9 };
const b = { x: 1.5 };
const c = { x: NaN };
console.log(a.x, b.x, c.x);
const ca: any = c;
console.log(Number.isNaN(ca.x));
function g(o: any) { return o.size; }
console.log(g({ size: 9, has: () => true, keys: () => [][Symbol.iterator]() }));
console.log(g({ size: NaN, has: () => true, keys: () => [][Symbol.iterator]() }));
console.log(g({ size: 1.5, has: () => true, keys: () => [][Symbol.iterator]() }));
console.log("done");
