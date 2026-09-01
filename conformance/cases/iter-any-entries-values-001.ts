// Iterator cells stepped through the any lane (for-of over
// .entries()/.values()/.keys() boxed into any) — value semantics
// hold and every yielded heap value carries exactly one stake (the
// surplus inc this lane kept after rotation 543's next()-twin fix
// leaked one pair array per entries step; 548-01).
const xs: any = ["ab", 7];
for (const [k, v] of xs.entries()) console.log(k, v);
for (const v of xs.values()) console.log(v);
for (const k of xs.keys()) console.log(k);
const nested: any = [[1], [2]];
for (const v of nested.values()) console.log(JSON.stringify(v));
const m: any = new Map();
m.set("k", 1);
m.set("j", [2]);
for (const [k, v] of m.entries()) console.log(k, JSON.stringify(v));
for (const k of m.keys()) console.log(k);
for (const v of m.values()) console.log(JSON.stringify(v));
const s: any = new Set(["a", "b"]);
for (const v of s.values()) console.log(v);
