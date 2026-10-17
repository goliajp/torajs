// `f.call(t, ...)` / `f.apply(t, [...])` on a function value hand the
// arguments after the receiver to its parameters; an object whose own
// methods are named `call` / `apply` takes every argument itself.
const o = {
  k: 2,
  call(t: number, x: number) {
    return t * x;
  },
  apply(t: number, xs: number[]) {
    return t * xs[0];
  },
};
console.log(o.call(3, 1.5), o.call(0.5, 4), o.apply(0.5, [3]));
function h(t: number, y: number) {
  return t + y;
}
const holder = { f: h };
const g = function (this: any, x: number) {
  return typeof this + ":" + x;
};
const args = [0.5, 1];
console.log(holder.f.call(null, 1, 2.5), h.apply(null, [0.5, 1]), h.apply(null, args));
console.log(g.call({}, 1.5), g.apply({}, [2.5]));
