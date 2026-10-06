// A method written in an object literal is a non-arrow
// function: it has its own `arguments`, also when the literal sits
// inside another function.
function f(z: number) {
  const o = {
    m(a: number, b: number) { return arguments.length + ":" + arguments[0] + "," + arguments[1]; },
    n: function (a: number) { return arguments.length + a; },
    k: () => arguments.length,
  };
  console.log(o.m(1, 2));
  console.log("fn field", o.n(10));
  console.log("arrow field", o.k(), z);
}
f(7);
class C {
  m(a: number) {
    const o = { q(x: number) { return arguments[0]; } };
    return o.q(a + 1);
  }
}
console.log(new C().m(4));
function idx(a: number, b: number) {
  const o = { m: function (x: number, y: number) { return arguments[0] + arguments[1] + arguments.length; } };
  return o.m(a, b);
}
console.log(idx(1, 2));
function outer() {
  const o = {
    inner() {
      const h = () => arguments.length;
      return h();
    },
  };
  return o.inner() + "/" + arguments.length;
}
console.log(outer.call(null, 1, 2, 3));
