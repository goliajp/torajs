// A field an object literal leaves out is no own property on any
// reflective surface, and a spread or Object.assign copies only the
// fields the source owns (§7.3.25 CopyDataProperties).

type O = { a?: number; s?: string; b: number };
const p: O = { b: 2 };
console.log(Object.keys(p).join(","), Object.values(p).length, Object.entries(p).length);
console.log(Object.hasOwn(p, "a"), p.hasOwnProperty("s"), Object.getOwnPropertyNames(p).join(","));
console.log(JSON.stringify(p), Reflect.ownKeys(p).length, Object.getOwnPropertyDescriptor(p, "a"));
for (const k in p) console.log("key", k);
console.log(p);

const t: O = { a: 7, s: "x", b: 0 };
Object.assign(t, p);
console.log(t.a, t.s, t.b);

const { a = 5, ...rest } = p;
console.log(a, Object.keys(rest).join(","), "s" in rest);

const d = { a: 1, s: "q", b: 9 };
const m = { ...d, ...p };
console.log(m.a, m.s, m.b, Object.keys(m).join(","));
const n = { ...p, ...d };
console.log(n.a, n.s, n.b, Object.keys(n).length);
const q = { ...p };
console.log(q.b, JSON.stringify(q), "a" in q, "b" in q);
const w = { ...p, c: 1 };
console.log(JSON.stringify(w), Object.keys(w).join(","));

const full: O = { a: 1, s: "y", b: 2 };
const u = { ...full };
console.log(JSON.stringify(u), Object.keys(u).join(","));

p.s = "now";
console.log(p.s, "s" in p, "a" in p, Object.keys(p).length);

const fz: O = { b: 1 };
Object.freeze(fz);
try {
  fz.a = 3;
  console.log("no throw");
} catch (e) {
  console.log("threw", e instanceof TypeError);
}
console.log(fz.a, "a" in fz);
