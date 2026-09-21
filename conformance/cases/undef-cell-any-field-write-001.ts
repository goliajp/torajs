// A struct field declared `any` stores a heap value as a raw
// pointer — the slot reads a pointer back and the reader tags it.
// That is right until the pointer IS the generic immortal cell: a
// pointer slot's in-band `undefined` is an address like any other,
// so `o.v = xs.find(p)` on a miss left `typeof o.v` answering
// "object" for a value that is `undefined`. The same expr-aware box
// the scalar and nullish-literal arms next to it already take
// settles it, branchlessly — a live value comes back out as itself.

const xs: number[][] = [];
const r = xs.find((z: number[]) => z.length > 99);
const a: number[] | undefined = undefined;

class Holder {
  v: any = 0;
  w: any = 0;
}
const h = new Holder();
h.v = r;
h.w = a;
console.log(typeof h.v, typeof h.w);
console.log(h.v === undefined, h.w === undefined);
console.log(String(h.v), JSON.stringify(h));

const o: { v: any } = { v: 0 };
o.v = r;
console.log(typeof o.v, o.v === undefined, JSON.stringify(o));

// --- a live value keeps the raw-store answers exactly
const live = [[1, 2], [3]];
const hit = live.find((z: number[]) => z.length > 1);
const h2 = new Holder();
h2.v = hit;
h2.w = [7, 8];
console.log(typeof h2.v, h2.v.length, h2.v.join("-"), h2.w.join("-"));
console.log(JSON.stringify(h2));

// --- and the payloads the neighbouring arms carry
const h3 = new Holder();
h3.v = 5;
console.log(typeof h3.v, h3.v + 1);
h3.v = 2.5;
console.log(typeof h3.v, h3.v * 2);
h3.v = true;
console.log(typeof h3.v, !h3.v);
h3.v = "s";
console.log(typeof h3.v, h3.v.length);
h3.v = undefined;
console.log(typeof h3.v, h3.v === undefined);
h3.v = null;
console.log(typeof h3.v, h3.v === null);
h3.v = { k: 1 };
console.log(typeof h3.v, h3.v.k);

// --- the field owns a balanced reference across a function edge
function make(): Holder {
  const inner = [[5, 6]];
  const found = inner.find((z: number[]) => z.length > 1);
  const b = new Holder();
  b.v = found;
  return b;
}
const kept = make();
console.log(kept.v.join("-"), typeof kept.v);
