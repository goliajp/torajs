// An out-of-range read stored in a field is still `undefined` when it
// comes back out. A `number` field has no type-level tell — it is
// seeded with 0 — so the sentinel gate has to know which field names
// a write ever hands one to.
const zs: number[] = [1, 2, 3];

const lit = { v: zs[9], w: zs[0] };
console.log(lit.v, lit.v === undefined, typeof lit.v);
console.log(lit.w, lit.w === undefined, typeof lit.w);

const later = { v: 0 };
later.v = zs[9];
console.log(later.v, later.v === undefined);

class K {
  f: number = 0;
}
const k = new K();
console.log(k.f, k.f === undefined);
k.f = zs[9];
console.log(k.f, k.f === undefined);

// Arithmetic on the way in is a plain NaN, not `undefined`, and the
// field round trip must not turn one into the other.
const arith = { v: zs[9] + 1 };
console.log(arith.v, arith.v === undefined, typeof arith.v);
