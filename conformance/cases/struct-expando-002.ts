// RFC 20260714-struct-dynamic-props blade 3 — enumeration faces over
// struct expandos: keys / for-in / values / entries / console.log /
// JSON.stringify, integer-first key order included.

class C {
  v: number = 2;
}
const o: any = new C();
o.a = 1;
o.s = "x";
console.log(Object.keys(o).join(","));

// integer keys merge ascending ahead of every string key
o[7] = 70;
o[3] = 30;
console.log(Object.keys(o).join(","));

// for-in sees the same order
const seen: string[] = [];
for (const k in o) seen.push(k);
console.log(seen.join(","));

// values / entries carry the expando values
const p: any = { w: 5 };
p.q = 6;
console.log(Object.values(p).join(","));
console.log(JSON.stringify(Object.entries(p)));

// console.log renders expandos after layout fields
const r: any = { m: 1 };
r.extra = "e";
console.log(r);

// JSON.stringify folds expandos in
console.log(JSON.stringify(r));

// empty-layout struct with expandos still prints its entries
function mkEmpty() {
  return {};
}
const e: any = mkEmpty();
e.only = 9;
console.log(JSON.stringify(e), Object.keys(e).join(","));

// getOwnPropertyNames includes expandos
console.log(Object.getOwnPropertyNames(p).join(","));
