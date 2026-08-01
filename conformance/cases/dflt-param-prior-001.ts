// defaults referencing PRIOR params in arrow / function-expression
// values materialize in the callee scope (§9.2 param order), not the
// caller's — the t262 dflt-params-ref-prior template family
var x = 0;
const a = (x, y = x, z = y) => {
  console.log(x, y, z);
};
a(3);
a(4, 7);
const f = function (p, q = p + 1) {
  console.log(p, q);
};
f(10);
f(10, 20);
// arg-val-undefined: an explicit undefined triggers the default too
const g = (m, n = m * 2) => {
  console.log(m, n);
};
g(5, undefined);
// trailing comma + compound default
const h = (u, v = u - 1,) => {
  console.log(u, v);
};
h(9);
// nested arrow inside an arrow body
const outer = () => {
  const inner = (s, t = s + 100) => s + t;
  return inner(1);
};
console.log(outer());
