// An alias names a type, not a spelling (TS spec §3.10): a union
// alias under an array suffix is an array of the union, the same type
// `(number | undefined)[]` spells, never `number | undefined[]`.

type U = number | undefined;
const xs: U[] = [1, undefined];
console.log(xs.length);

const ys: readonly U[] = [2, undefined, 3.5];
console.log(ys.length, ys[2]);

type Row = U[];
const r: Row = [undefined, 4.5];
r.push(0.25);
console.log(r.length, r[1], r[2]);

type V = U;
const vs: V[] = [1.5];
console.log(vs[0]);

type S = number | string;
const ss: S[] = [1, "a"];
ss.push(2.5);
console.log(ss, typeof ss[1]);

function total(a: U[]): number {
  let t = 0;
  for (const v of a) {
    if (v !== undefined) t += v;
  }
  return t;
}
console.log(total([0.5, undefined, 1.25]));

type Grid = Row[];
const g: Grid = [[1.5], [undefined, 2.5]];
console.log(g.length, g[1][1]);
