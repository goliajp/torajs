// a fn-typed parameter that only ever receives named functions, handed
// to an `any` slot: an annotated binding, an assignment, an `any`
// return, an `any` parameter, a choice the checker joins to any
function inc(n: number): number { return n + 1; }
function dbl(n: number): number { return n * 2; }
function bind(f: (n: number) => number, c: boolean) {
  const g: any = c ? 0 : f;
  console.log(typeof g, g === 0 ? "zero" : g(3), g === inc);
}
bind(inc, false);
bind(inc, true);
bind(dbl, false);
function plain(f: (n: number) => number) {
  const g: any = f;
  return typeof g + " " + g(4);
}
console.log(plain(inc), plain(dbl));
function later(f: (n: number) => number) {
  let g: any = 0;
  g = f;
  return g(5);
}
console.log(later(inc));
function out(f: (n: number) => number, c: boolean): any {
  return c ? 0 : f;
}
console.log(out(dbl, false)(6), out(dbl, true));
function kind(x: any): string { return typeof x; }
function relay(f: (n: number) => number) { return kind(f); }
console.log(relay(inc));
function inferred(f: (n: number) => number, c: boolean) {
  const k = c ? "none" : f;
  return typeof k;
}
console.log(inferred(inc, false), inferred(inc, true));
function viaConst(f: (n: number) => number) {
  const k = f;
  const g: any = k;
  return g(7);
}
console.log(viaConst(dbl));
function maybe(f: (n: number) => number, c: boolean) {
  const g: any = c ? f : null;
  return g === null ? "null" : g(8);
}
console.log(maybe(inc, true), maybe(inc, false));
// the parameter still calls directly where it is not boxed
function apply(f: (n: number) => number, n: number): number { return f(n); }
let t = 0;
for (let i = 0; i < 1000; i++) t += apply(i % 2 ? inc : dbl, i) + later(inc);
console.log(t);
