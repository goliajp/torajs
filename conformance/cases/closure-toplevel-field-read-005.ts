// a method field read off a top-level literal as a value is the
// function itself: a call through the binding has an undefined this
let src = {
  n: 5,
  m(x: number) { return x + 1 },
  t() { return this === undefined },
  r() { try { return this.n } catch (e) { return e instanceof TypeError } },
};
const g = src.m;
const t = src.t;
const r = src.r;
function call() { return g(1) + " " + t() + " " + r() }
console.log(call(), src.n, src.t(), src.r());
