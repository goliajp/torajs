// includes reads a hole of a number[] as undefined (§23.1.3.16 has no
// HasProperty step): includes(undefined) finds it, includes(NaN) does not.
let a = [1.5, 2.5];
a[4] = 3.5;
console.log(a.includes(undefined as any), a.includes(NaN), a.includes(3.5), a.includes(0));
console.log(a.indexOf(undefined as any), a.lastIndexOf(undefined as any));
const n = [NaN, 1.5];
console.log(n.includes(NaN), n.includes(undefined as any), n.indexOf(NaN));
const d = [1.5, 2.5];
console.log(d.includes(undefined as any), d.includes(2.5));
let g = [0.5];
g.length = 2;
console.log(g.includes(undefined as any), g.includes(0));
const zs: number[] = [1.5];
console.log(a.includes(zs[9]), d.includes(zs[9]), n.includes(zs[9]));
