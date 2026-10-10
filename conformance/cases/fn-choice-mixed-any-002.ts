// a choice mixing a named function with a non-function value: the
// function joins as its own fn object in every position
function inc(n: number): number { return n + 1; }
function dec(n: number): number { return n - 1; }
function twice(n: number): number { return n * 2; }
const pickN = (k: number) => (k > 1 ? (k > 2 ? inc : 7) : k > 0 ? "s" : twice);
for (const k of [0, 1, 2, 3]) {
  const v: any = pickN(k);
  console.log(k, typeof v, typeof v === "function" ? v(10) : v, v === inc, v === twice);
}
function show(x: any): string { return typeof x === "function" ? "fn:" + x(3) : "val:" + x; }
const off = Math.random() > 2;
console.log(show(off ? 0 : dec), show(off || dec), show(!off && "no"), show(off && inc));
function choose(useInc: boolean): (n: number) => number {
  return (useInc && inc) || dec;
}
console.log(choose(true)(1), choose(false)(1), choose(true) === inc);
const arr = [off ? 1 : inc, off ? "x" : twice, 5];
console.log(arr.map((f: any) => (typeof f === "function" ? f(4) : f)).join(","));
let none: ((n: number) => number) | null = null;
const viaNullish = none ?? twice;
const viaOr = none || dec;
console.log(viaNullish(6), viaNullish === twice, viaOr(6), viaOr === dec);
const marked: any = off ? 0 : inc;
marked.tag = "t";
console.log((inc as any).tag, marked === inc);
// every evaluation hands out the same object; none of them releases it
let acc = 0;
for (let i = 0; i < 100000; i++) {
  const f: any = i < 0 ? 0 : inc;
  acc = f(acc);
  const g = (i < 0 && inc) || dec;
  acc = g(acc) + 1;
  const h = none ?? twice;
  acc = h(acc) / 2;
}
console.log(acc, (off ? 0 : inc) === inc, (inc as any).tag);
