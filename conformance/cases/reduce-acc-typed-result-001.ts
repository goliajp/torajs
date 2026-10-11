// A seeded reducer whose accumulator differs from the element type
// answers the accumulator's type; a callback return or a seed typed
// `any` reaches it through a cast into that type.
type Acc = { sum: number; n: number };
const xs: number[] = [1, 2, 3, 4];

const a = xs.reduce((acc: { sum: number }, x: number) => ({ sum: acc.sum + x }), { sum: 0 });
const total: number = a.sum;
console.log(total);

const b = xs.reduce((acc: string, x: number) => acc + x, "");
console.log(b.length, b);

const c = xs.reduce((acc: number[], x: number) => {
  acc.push(x * 2);
  return acc;
}, []);
console.log(c.length, c[3]);

const d = xs.reduce((acc: Acc, x: number) => ({ sum: acc.sum + x, n: acc.n + 1 }), { sum: 0, n: 0 });
console.log(d.sum / d.n);

const e = xs.reduce((acc: Acc, x: number): any => ({ sum: acc.sum + x, n: acc.n + 1 }), { sum: 0, n: 0 });
console.log(e.sum, e.n);

const seed: any = { sum: 100 };
const f = xs.reduce((acc: { sum: number }, x: number) => ({ sum: acc.sum + x }), seed);
console.log(f.sum);

const seedArr: any = [9];
const g = xs.reduce((acc: number[], x: number) => {
  acc.push(x);
  return acc;
}, seedArr);
console.log(g.length, g[0] + g[4]);

const h = xs.reduceRight((acc: string, x: number) => acc + x + ",", ">");
console.log(h);

const empty: number[] = [];
const i = empty.reduce((acc: { sum: number }, x: number) => ({ sum: acc.sum + x }), seed);
console.log(i.sum);

const j = xs.reduce((acc: boolean, x: number) => acc && x > 0, true);
console.log(j);
