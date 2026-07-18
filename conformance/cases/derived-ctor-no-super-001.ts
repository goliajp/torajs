// RFC 20260718-error-message-own-prop 刀 3 — §9.2.2 [[Construct]]
// this-TDZ: a derived ctor that never calls super() throws a
// ReferenceError at its implicit return; body side effects still run
// first. A ctor WITH super() constructs normally.
class A {}
class B extends A {
  constructor() {
    console.log("side");
  }
}
try {
  new B();
} catch (e: any) {
  console.log("caught:", e.name, "|", e.message);
}
class C extends TypeError {
  constructor() {}
}
try {
  new C();
} catch (e: any) {
  console.log("caught2:", e.name);
}
class D extends A {
  constructor() {
    super();
  }
}
console.log(new D() instanceof D);
