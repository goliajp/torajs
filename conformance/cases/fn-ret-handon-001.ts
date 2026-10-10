// a fn-typed return that hands on what a function retagged to the
// closure lane returns, directly or through a const binding, takes the
// lane too
const objs: any = { f: (n: number) => n + 1 };
function dbl(n: number): number { return n * 2; }
function pick(k: number): (n: number) => number {
  if (k > 0) return dbl;
  const v = objs.f;
  return v;
}
function wrap(k: number): (n: number) => number { return pick(k); }
function wrap2(k: number): (n: number) => number {
  const r = pick(k);
  return r;
}
function wrap3(k: number): (n: number) => number { return k > 5 ? wrap(k) : wrap2(k); }
console.log(pick(0)(1), pick(1)(1));
console.log(wrap(0)(1), wrap(1)(1));
console.log(wrap2(0)(1), wrap2(1)(1), wrap(1) === dbl, wrap3(9) === dbl, wrap3(0)(3));
let t = 0;
for (let i = 0; i < 1000; i++) t += wrap3(i % 11)(i);
console.log(t);
