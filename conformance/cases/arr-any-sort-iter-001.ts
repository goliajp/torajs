// any-method dispatch backfill chunk 4 — Array.prototype.sort /
// keys / values / entries through an `any` receiver (closing the
// 15-method Array backfill). sort rides the stable merge-sort
// kernel through two new ANY modes: a user comparator crosses the
// boxed dual-entry ABI, its absence is the §23.1.3.30.2 step-10
// default (undefined-last + ToString UTF-16 order); typed blocks
// behind the any view rebox per elem kind at compare time only.
// The iterator trio mints ArrIter cells whose step now does the
// kind-aware borrowed read (both receiver shapes).
//
// Acceptance: byte-equal with bun.

// default sort — ToString order, incl. the classic [10, 9, 1] trap
const a = [10, 9, 1, 33, 2] as any;
console.log(a.sort());
console.log(a.sort((x: any, y: any) => x - y));
console.log(a.sort((x: any, y: any) => y - x));

// heap elements + mixed Arr<Any>
const s: any = ["pear", "apple", "fig"];
console.log(s.sort());
const m: any = [3, "1", true, 20];
console.log(m.sort());

// undefined sorts last, comparator never sees one
const u: any = [3, undefined, 1, undefined, 2];
console.log(u.sort());
console.log(u.sort((x: any, y: any) => x - y));

// chaining answer + empty / single edges
const ch = [2, 1] as any;
console.log(ch.sort().reverse());
console.log(([] as any).sort());
console.log(([5] as any).sort((x: any, y: any) => x - y));

// keys / values / entries — Arr<Any> receiver
const k: any = ["a", "b", "c"];
for (const i of k.keys()) console.log(i);
for (const v of k.values()) console.log(v);
for (const e of k.entries()) console.log(e);

// typed receiver via as-cast (kind-aware iter step)
const tk = [7, 8] as any;
for (const i of tk.keys()) console.log(i);
for (const v of tk.values()) console.log(v);
for (const e of tk.entries()) console.log(e);
