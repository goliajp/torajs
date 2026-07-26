// `fill` (§23.1.3.7 step 12) and `copyWithin` (§23.1.3.4 step 15) mutate
// in place and answer `this` — the product IS the receiver, the same way
// `valueOf`'s is.
//
// The width analysis had no arm for either name, so the call answered
// "no key" and whatever held the result never joined the receiver's
// class. Widening either side then left the other one compiled as
// integers over the very same slots, and the read reinterpreted the
// bits — silently, in both directions.

const xs: number[] = [1, 2, 3, 4, 5];
const ys: number[] = xs.fill(0, 1, 4);
ys[4] = 1.5;
console.log(xs[0], xs[4], ys[0], ys[4]);

// the other direction — receiver widened before the call
const as: number[] = [1, 2, 3, 4, 5];
as[4] = 2.5;
const bs: number[] = as.fill(0, 1, 4);
console.log(bs[0], bs[4]);

// copyWithin, both directions
const cs: number[] = [1, 2, 3, 4, 5];
const ds: number[] = cs.copyWithin(0, 3);
ds[4] = 3.5;
console.log(cs[0], cs[1], cs[4], ds[0], ds[4]);

const es: number[] = [1, 2, 3, 4, 5];
es[4] = 4.5;
const gs: number[] = es.copyWithin(0, 3);
console.log(gs[0], gs[1], gs[4]);

// the fill value itself widens the element class
const hs: number[] = [1, 2, 3];
const is: number[] = hs.fill(0.5, 1);
console.log(hs[0], hs[1], is[0], is[1]);

// a method seed widens it instead of a write
const js: number[] = [6, 7, 8];
js.find((x: number): boolean => x > 7);
const ks: number[] = js.copyWithin(0, 2);
console.log(ks[0], ks[2]);

// all-integral receivers stay narrow and unaffected
const ns: number[] = [10, 11, 12];
const nv: number[] = ns.fill(9, 2);
console.log(nv[0], nv[2]);
const ms: number[] = [13, 14, 15];
const mv: number[] = ms.copyWithin(0, 1);
console.log(mv[0], mv[2]);

// strings never carry a width, and the trailing-arg shape still holds
const ss: string[] = ["a", "b", "c", "d"];
console.log(ss.fill("z", 0, 2)[0], ss.copyWithin(2, 0)[2]);
