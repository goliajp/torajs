// An untyped-param user fn taken as a value synthesizes a
// `__forward_*` shim whose cloned params carry no annotation; the
// implicit-generics pass must Any-default them like any other
// `__env`-first closure shape (it used to gate on `__closure_*`
// only, so the whole program rejected with "parameter `n` of
// function `__forward_g` requires a type annotation").

function g(n) {
  return n * 2;
}
const t: any = g;
console.log(t(21)); // 42

// destructured untyped param
function f({ a }) {
  return a;
}
const u: any = f;
console.log(u({ a: 3 })); // 3

// multi-param
function h(x, y) {
  return x + y;
}
const v: any = h;
console.log(v(1, 2)); // 3

// direct calls to the targets keep their own lane
console.log(g(5), h(2, 3)); // 10 5
console.log("done");
