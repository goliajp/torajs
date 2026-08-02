// RFC 20260802-class-computed-member 刀 3 后半 — instance + static
// computed FIELD names: key evaluates once at class-definition time,
// instance init evaluates per construction, both read back through
// keyed access.

let tick = 0;
function nextKey(): string {
  tick = tick + 1;
  return "k" + tick;
}

class A {
  [nextKey()] = 10;
  static [nextKey()] = 20;
  ["lit" + "eral"] = 30;
  [2 + 2] = 40;
}

// key expr ran once per ComputedPropertyName, in declaration order
console.log(tick);

const a1: any = new A();
const a2: any = new A();
console.log(a1.k1, a1.literal, a1[4]);
console.log(a2.k1);
const AC: any = A;
console.log(AC.k2);

// per-instance: writes on one instance do not leak to the other
a1.k1 = 99;
console.log(a1.k1, a2.k1);

// init evaluates per construction (fresh object each time)
class B {
  [nextKey()] = { n: 0 };
}
const b1: any = new B();
const b2: any = new B();
b1.k3.n = 7;
console.log(b1.k3.n, b2.k3.n);

// init referencing this (fields defined in order)
class D {
  base = 5;
  ["deriv" + "ed"] = this.base + 1;
}
const d: any = new D();
console.log(d.derived);

// enumeration order: computed fields appear among own keys
console.log(Object.keys(a1).join(","));
