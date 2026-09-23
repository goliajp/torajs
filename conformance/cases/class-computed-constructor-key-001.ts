// §15.7.2 ConstructorMethod reads PropName, and §8.2 says PropName of
// a ComputedPropertyName is EMPTY. So `["constructor"]() {}` is never
// the class's constructor, however the expression evaluates: it is an
// ordinary prototype method that lands on the "constructor" key at run
// time, overwriting the one ClassDefinitionEvaluation installed.

class A {
  constructor() { (this as any).k = 1; }
  ["constructor"]() { return "M"; }
}
const a: any = new A();
console.log("A", a.k, typeof a.constructor, a.constructor());

// Source order does not matter — the computed one is not a duplicate
// of the constructor, it is a later definition of the key.
class B {
  ["constructor"]() { return "M"; }
  constructor() { (this as any).k = 2; }
}
const b: any = new B();
console.log("B", b.k, b.constructor());

// The key can come from anywhere; it is still computed.
const key = "constructor";
class C {
  [key]() { return "E"; }
  constructor() { (this as any).v = 9; }
}
const c: any = new C();
console.log("C", c.v, c.constructor());

// PrototypePropertyNameList collects only the non-static elements, so
// a static `constructor` was never the constructor either.
class D {
  static constructor() { return "S"; }
}
console.log("D", (D as any).constructor());
