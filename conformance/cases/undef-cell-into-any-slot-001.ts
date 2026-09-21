// A pointer slot's in-band `undefined` is a real address. Pushing
// one into an `any` element stamped ANY_HEAP over it, so the slot
// held a heap tag pointing at the immortal cell and `typeof b[0]`
// answered "object" for a value that is `undefined`. The tag is a
// runtime question here — 4 + (val == sentinel) — which is the
// encoding `box_to_any_from_expr` has used for this family since
// it existed; the grow-mutator pack was the copy that never asked.

const xs: number[][] = [];
const r = xs.find((z: number[]) => z.length > 99);
const a: number[] | undefined = undefined;

const b: any[] = [];
b.push(r);
b.unshift(a);
console.log(typeof b[0], typeof b[1]);
console.log(b[0] === undefined, b[1] === undefined);
console.log(String(b[0]), JSON.stringify(b));

// --- the index-assign door is the same pack under another name
// (`pack_any_slot_value_shared`, eight call sites): a static key, a
// grow-on-write past the end, and a computed key.
const e: any[] = [0, 0];
e[0] = r;
e[1] = a;
e[5] = r;
let k = 0;
k = k + 1;
e[k + 1] = r;
console.log(typeof e[0], typeof e[1], typeof e[5], typeof e[2]);
console.log(e[0] === undefined, e[5] === undefined, e[2] === undefined);
console.log(JSON.stringify(e));

// --- a live value through the same door keeps its own answers
const live = [[1, 2], [3]];
const hit = live.find((z: number[]) => z.length > 1);
const c: any[] = [];
c.push(hit);
c.unshift([9]);
console.log(typeof c[0], typeof c[1], c[1].length, c[1].join("-"));
console.log(JSON.stringify(c));
const f: any[] = [0];
f[0] = hit;
f[3] = [8];
console.log(typeof f[0], f[0].join("-"), f[3].join("-"), typeof f[1]);

// --- and every other payload the pack knows
const d: any[] = [];
d.push(1);
d.push(2.5);
d.push(true);
d.push("s");
d.push(undefined);
d.push(null);
d.push({ k: 1 });
console.log(d.map((v: any) => typeof v).join(","));
console.log(JSON.stringify(d));

// --- the slot owns a balanced reference: the array outlives the
// binding that put the value there, and reading it back still works
function make(): any[] {
  const inner = [[5, 6]];
  const found = inner.find((z: number[]) => z.length > 1);
  const out: any[] = [];
  out.push(found);
  return out;
}
const kept = make();
console.log(kept[0].join("-"), kept.length);
