// §14.3.1 — a BindingPattern is a declarator like any other: it may sit
// at any position of a `let` / `const` / `var` list, not only alone.
const [a, b] = [1, 2], [c, d] = [3, 4];
let x = 10, [y, z] = [20, 30], { p, q: r } = { p: 40, q: 50 }, w = 60;
var { m } = { m: 7 }, n = 8, [o = 9] = [] as number[];
console.log(a, b, c, d, x, y, z, p, r, w, m, n, o);
function f() {
  const [u] = [2], [v] = [u + 1];
  return u * 10 + v;
}
console.log(f());
let s = 0;
for (let i = 0, [lo, hi] = [1, 3]; i < 2; i++) s += lo + hi + i;
console.log(s);
const [e] = [5]
const g = 6
console.log(e + g);
