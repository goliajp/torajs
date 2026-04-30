// Adapted from test262: 3-level inheritance — A → B → C. Verifies
// `super(args)` plumbs through multiple levels and the flattened field
// list contains every ancestor's fields in declaration order.
class A {
  a: number;
  constructor(a: number) { this.a = a; }
  describeA(): number { return this.a; }
}

class B extends A {
  b: number;
  constructor(a: number, b: number) {
    super(a);
    this.b = b;
  }
  describeB(): number { return this.b; }
}

class C extends B {
  c: number;
  constructor(a: number, b: number, c: number) {
    super(a, b);
    this.c = c;
  }
  describeC(): number { return this.c; }
}

function check(): number {
  let x = new C(1, 2, 3);
  if (x.a !== 1) { throw "#1"; }
  if (x.b !== 2) { throw "#2"; }
  if (x.c !== 3) { throw "#3"; }
  if (x.describeA() !== 1) { throw "#4"; }
  if (x.describeB() !== 2) { throw "#5"; }
  if (x.describeC() !== 3) { throw "#6"; }
  return 0;
}
console.log(check());
