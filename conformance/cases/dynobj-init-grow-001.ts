// RFC 20260721-array-proto-cluster 刀 8-B chunk 3 — object-literal
// dynobj init across the table resize boundary. The dense entry
// array starts at entries_cap 7; the 8th field's insert relocates
// the block (fresh alloc + free of the old one). Pre-fix every set
// used a throwaway relocation slot, so fields from the 8th on landed
// in a freed orphan and the literal's result kept the dangling
// pre-resize pointer (use-after-free silent-wrong).

const o: any = {};
const r: any = { length: 10, 0: undefined, 1: 2, 2: 1, 3: "X", 4: -1, 5: "a", 6: true, 7: o, 8: NaN, 9: Infinity };
console.log("keys:", Object.keys(r).join(","));
for (let i = 0; i < 10; i++) console.log(i, i in r, String(r[i]));
console.log("len:", r.length, "obj7:", r[7] === o);

// borrowed generic sort over the full literal (the shape that first
// exposed the loss — sm sort A3_T1/T2).
const s: any = { length: 4, 0: 3, 1: "b", 2: true, 3: 1, x1: 0, x2: 0, x3: 0, x4: 0, x5: 0 };
s.sort = Array.prototype.sort;
s.sort();
console.log("sorted:", String(s[0]), String(s[1]), String(s[2]), String(s[3]));

// accessor shorthand as the capacity-filling field — the define
// kernel's resize must write back through the shared init slot too.
const a: any = { f0: 0, f1: 1, f2: 2, f3: 3, f4: 4, f5: 5, f6: 6, get g() { return 99; }, f8: 8 };
console.log("acc:", a.g, a.f8, a.f0, Object.keys(a).length);

// nested literal past the boundary keeps identity.
const n: any = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: { deep: true }, i: 9 };
console.log("nested:", n.h.deep, n.i, Object.keys(n).length);
