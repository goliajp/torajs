// A for-of variable over a number[] with holes (a write past the end, a
// longer length) reads the hole as undefined — typeof, ===, String.
let a = [1.5, 2.5];
a[4] = 3.5;
const r: string[] = [];
for (const v of a) r.push(String(v));
console.log(r.join(","));
for (const v of a) console.log(typeof v, v === undefined);
let g = [7.5];
g.length = 3;
let n = 0;
for (const v of g) if (v === undefined) n++;
console.log(n);
const d = [1.5, 2.5];
d[1] = 4.5;
let s = 0;
for (const v of d) s += v;
console.log(s);
