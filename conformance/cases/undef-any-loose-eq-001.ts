// chunk 619 — nullish-typed binding x any loose eq (last 612-era
// admit face): checker admits, S127-2 lowering probes the any side
// for null-or-undefined.
let u = undefined;
const n = null;
const a: any = 5;
console.log(u === a, a === u, u == a, a == u);
console.log(n == a, a == n);
const b: any = undefined;
console.log(u == b, b == u, n == b);
const c: any = null;
console.log(u == c, c == u, n == c);
