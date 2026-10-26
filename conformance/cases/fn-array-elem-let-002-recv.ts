// An annotated function binding initialized by an element of a function
// array whose array expression is not a local: a field read through
// `this` or another receiver, and a call's result.
type F = (x: number) => number;
const k = 2;
class B {
  fs: F[] = [(x: number) => x + 1, (x: number) => x * 3];
  go(i: number): number {
    const f: F = this.fs[i];
    return f(5);
  }
}
const b = new B();
console.log(b.go(0), b.go(1));
const o = { fs: [(x: number) => x - k] as F[] };
const g: F = o.fs[0];
console.log(g(10));
const g2: F = b.fs[1];
console.log(g2(7));
function mk(): F[] {
  return [(x: number) => x * 10 + k];
}
const h: F = mk()[0];
console.log(h(2));
let m: F = b.fs[0];
m = mk()[0];
console.log(m(1));
