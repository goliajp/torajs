// chunk 706 — Object.values / Object.entries on an any-typed dynobj
// receiver (594 keys-family completion): live entry walk in ES order,
// enumerable-only, accessor entries run the getter, entries feeds
// fromEntries (outer array elem-kind stamped).
const d: any = { b: 2, a: 1, s: "hi" };
console.log(Object.values(d));
console.log(Object.keys(d));
console.log(Object.entries(d));
for (const [k, v] of Object.entries(d)) console.log(k, v);
d["10"] = "ten"; d["2"] = "two";
console.log(Object.keys(d));
console.log(Object.entries(d));
let got = 0;
Object.defineProperty(d, "g", { get: () => { got++; return 99; }, enumerable: true });
console.log(Object.values(d));
console.log(Object.entries(d).length, "got", got);
Object.defineProperty(d, "hidden", { value: 7, enumerable: false });
console.log(Object.keys(d).length, Object.values(d).length, Object.entries(d).length);
const o = { x: 1, y: "s" };
const oa: any = o;
console.log(Object.values(oa));
console.log(Object.entries(oa));
const rt: any = Object.fromEntries(Object.entries({ p: 1, q: "z" } as any));
console.log(rt.p, rt.q);
