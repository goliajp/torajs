// a class method read as a value and stored in a container literal:
// the element / field is the shared prototype function object, and a
// call through the container runs it
class D {
  n(x: number) { return x * 10; }
  two(a: number, b: number) { return a - b; }
}
const d = new D();
console.log([d.n][0](2), [d.n].length);
const m = [d.n, d.two];
console.log(m[0](3), m[1](9, 4), m.length, m[0] === d.n);
const o = { f: d.n, g: d.two };
console.log(o.f(2), o.g(7, 2), o.f === d.n);
console.log(typeof m[0], typeof o.f);
