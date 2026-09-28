// a class method read as a value and stored in a top-level container
// literal, then reached from a named function: the binding is a
// global whose element / field holds the shared prototype function
// object, and a call through the container runs it with the
// container as `this`
class D {
  n(x: number) { return x * 10; }
  two(a: number, b: number) { return a - b; }
}
class E extends D {}
const d = new D();
const e = new E();
const m = [d.n, d.two, e.n];
const o = { f: d.n, g: d.two, h: e.two };
function f() { return m[0](5) + m[1](9, 4) + m[2](1); }
function g() { return o.f(4) + o.g(7, 2) + o.h(3, 1); }
console.log(f(), g());
console.log([d.n].map(h => h(1)));
console.log(typeof m[0], typeof o.f, m.length, m[2] === d.n, o.h === d.two);
