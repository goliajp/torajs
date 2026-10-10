// the closure-lane return handed to a fn-typed parameter, relayed into
// another such parameter and called through a function boxed into any
const objs: any = { f: (n: number) => n + 1 };
function dbl(n: number): number { return n * 2; }
function pick(k: number): (n: number) => number {
  if (k > 0) return dbl;
  const v = objs.f;
  return v;
}
function apply(f: (k: number) => (n: number) => number, k: number): number {
  return f(k)(5);
}
function outer(g: (k: number) => (n: number) => number, k: number): number {
  return apply(g, k) + 100;
}
console.log(outer(pick, 1), outer(pick, 0));
const ap: any = apply;
console.log(ap(pick, 1));
