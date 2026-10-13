// A function declared to return a number hands back, as it is, a value
// read through a getter declared to return `any`, on a class (and a
// subclass that inherits it) and on an object literal, and a value read
// from a parameter annotated `unknown`.
class D {
  get v(): any { return "d" }
}
class E extends D {}
function dv(d: D): number { return d.v }
console.log(typeof dv(new D()) + ":" + dv(new D()), dv(new E()));
function ev(e: E): number { return e.v }
console.log(ev(new E()));
const o = { get w(): any { return "o" } };
const ow = (): number => o.w;
console.log(typeof ow() + ":" + ow());
function u(x: unknown): number {
  const y: any = x;
  return y;
}
console.log(typeof u("s") + ":" + u("s"), u(7) + 1);
