// RFC 20260808-construct-channel B6 刀 1 — Array.from through a
// detached value runs the real §23.1.2.1 any-tier kernel (was a loud
// reject): array-like index reads, the exact «kValue, k» mapfn call
// shape, string/array iterable sources, and the step-2 non-callable
// mapfn TypeError.
const list: any = { '0': 41, '1': 42, '2': 43, length: 3 };
const f: any = Array.from;
const a: any = f(list);
console.log(a.length, a[0], a[1], a[2]);
const b: any = f(list, function (v: any, k: any) { return v * 2 + k; });
console.log(b.length, b[0], b[1], b[2]);
const c: any = f("abc");
console.log(c.length, c[0], c[2]);
const d: any = f([7, 8, 9]);
console.log(d.length, d[0], d[2]);
try { f([1], 5); } catch (e) { console.log("caught"); }
