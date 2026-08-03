// TS this-parameter — `function f(this: T, ...)` declares the type of
// `this` only; it produces no runtime parameter. Legal in first
// position of fn-decls and fn-exprs.
const xs = [1, 2];
const ctx = { k: 100 };
function addK(this: any, v: number) {
  return [v + this.k];
}
const r = xs.flatMap(addK, ctx);
console.log(r.length, r[0], r[1]);
const r2 = xs.map(function (this: any, v: number) {
  return v + this.k;
}, ctx);
console.log(r2[0], r2[1]);
function plain(this: void) {
  return 5;
}
console.log(plain());
