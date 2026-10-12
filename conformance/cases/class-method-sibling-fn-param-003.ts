// the same method name on unrelated classes, reached as a static
// method, through `any`, and with a callback of another arity
class A {
  static run(f: (x: number) => number) { return f(2) }
  m(f: (x: number) => number) { return f(3) }
}
class B {
  static run(f: (x: number) => number) { return f(4) + 1 }
  m(f: (x: number) => number) { return f(5) + 1 }
}
const base = 10
console.log(A.run((x) => x * base), B.run((x) => x * base))
const a: any = new A()
const b: any = new B()
console.log(a.m((x: number) => x + base), b.m((x: number) => x + base))
function dbl(x: number): number { return x * 2 }
console.log(new A().m(dbl), new B().m(dbl), A.run(dbl), B.run(dbl))
const pick = (o: A | B) => o.m((x) => x - base)
console.log(pick(new A()), pick(new B()))
