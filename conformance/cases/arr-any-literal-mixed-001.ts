// T-10.d close — array literals mixing non-literal heap elements
// with scalars classify by checker type and route to the Array<Any>
// path. Pre-fix the non-literal elements were skipped by the
// AST-shape probe, the literal anchored on the scalar's kind, and
// the heap pointer + scalar shared one typed 8-byte slot
// interpretation (index reads returned null / bare pointer digits /
// denormal f64s; scope-end drop walked garbage). Assertions read
// slots back by index — whole-array print goes through the separate
// inspect-layout trunk.

// inline anon struct + int.
const a1 = [{ k: 1 }, 2];
console.log(a1[1]);
console.log(a1[0].k);
console.log(a1.length);

// ident-bound struct + int, both orders.
const o = { k: 5 };
const a2 = [o, 2];
console.log(a2[1]);
const a3 = [2, o];
console.log(a3[0]);
console.log(a3[1].k);

// class instance + int.
class K {
  k: number;
  constructor() {
    this.k = 42;
  }
}
const a4 = [new K(), 2];
console.log(a4[1]);
console.log(a4[0].k);

// heap string variable between scalars.
const s = "heap-string-payload-longer-than-cap-aa";
const a5 = [1, s, 3.5];
console.log(a5[0]);
console.log(a5[1]);
console.log(a5[2]);

// Map / Date / RegExp + int.
const m = new Map();
m.set("x", 9);
const a6 = [m, 2];
console.log(a6[1]);
console.log(a6[0].get("x"));
const a7 = [new Date(5), 2];
console.log(a7[1]);
console.log(a7[0].getTime());
const a8 = [/ab+/, 2];
console.log(a8[1]);
console.log(a8[0].source);

// homogeneous fast paths stay typed (ident among scalars, arr of arrs).
const x = 7;
const a9 = [1, x, 3];
console.log(a9[1]);
const xs = [1, 2];
const a10 = [xs, 3];
console.log(a10[1]);
console.log(a10[0][0]);
