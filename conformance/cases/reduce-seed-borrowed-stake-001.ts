// a reduce seed held by a binding keeps that binding's own reference:
// the accumulator slot retains it instead of borrowing the binding's
function arrSeed(ys: number[]): number[] {
  const seed: number[] = [];
  const r = ys.reduce((acc: number[], x: number) => { acc.push(x * 2); return acc }, seed);
  console.log(r.length);
  return seed;
}
const a = arrSeed([1, 2, 3]);
const d: number[] = [];
d.push(5);
d.push(5);
console.log(a.join(","), d.join(","));
a.push(11);
console.log(a.join(","), d.join(","));

function strSeed(ys: number[], n: number): string {
  const seed: string = "s" + n;
  const r = ys.reduce((acc: string, x: number) => acc + x, seed);
  console.log(r);
  return seed;
}
const s = strSeed([1, 2, 3], 7);
const t = "t" + 9;
console.log(s, t);

function anySeed(ys: any[]): any {
  const seed: any = [ys.length];
  const r = ys.reduce((acc: any, x: any) => { acc.push(x); return acc }, seed);
  console.log(r.length);
  return seed;
}
const c = anySeed([1, 2]);
const e: any = [5, 5, 5, 5];
console.log(c.join(","), e.join(","));

type P = { n: number };
function objSeed(ys: number[]): P {
  const seed: P = { n: 0 };
  const r = ys.reduceRight((acc: P, x: number) => { acc.n = acc.n * 10 + x; return acc }, seed);
  console.log(r.n, r === seed);
  return seed;
}
const o = objSeed([1, 2, 3]);
const f: P = { n: 99 };
console.log(o.n, f.n);

let total = 0;
for (let i = 0; i < 2000; i++) {
  const seed: number[] = [i];
  const r = [1, 2].reduce((acc: number[], x: number) => { acc.push(x); return acc }, seed);
  total += r.length + seed[0];
}
console.log(total);
