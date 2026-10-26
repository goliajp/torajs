// An `any` function stored into an element of a typed function array
// keeps its own face: a call through the element answers what the
// stored function returns, whatever its width or arity.
const quarter: any = (x: any) => x / 4;
const tag: any = (x: any) => "g" + x;
const pair: any = (a: any, b: any) => a + "," + b;

const hs: ((x: number) => number)[] = [(x: number) => x + 1, (x: number) => x * 2];
console.log(hs[0](3), hs[1](3));
hs[0] = quarter;
hs[1] = tag;
console.log(hs[0](3) + 1);
console.log(hs[1](3) + 1);
const k: number = hs[0](3);
console.log(k * 2);
let t = 0;
for (let i = 0; i < 2; i++) t = t + hs[0](i);
console.log(t);

function run(fs: ((x: number) => number)[], v: number): number {
  return fs[0](v) + fs[1](v);
}
console.log(run(hs, 10));

// literal elements
const ls: ((x: number) => number)[] = [tag, quarter, pair, (x: number) => x];
for (let i = 0; i < ls.length; i++) console.log(ls[i](5));
console.log(typeof ls[1](2), typeof ls[0](2));
