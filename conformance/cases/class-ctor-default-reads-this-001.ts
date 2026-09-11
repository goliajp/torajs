// §10.2.11 + §10.2.2 [[Construct]] — a base-class constructor's
// parameter defaults evaluate after the instance's fields are
// initialized and with `this` bound to it. The class factory
// carried the constructor's defaults on its own parameter list, so
// a default reading `this` was materialized in the factory — the
// one function where `this` is a local declared later — and the
// checker refused the second `__this`. The factory now hands such a
// parameter through untouched and the constructor evaluates it.
class A {
  #x = "hello";
  constructor(o: any = this.#x) {
    (this as any).value = o;
  }
}
console.log((new A() as any).value, (new A("given") as any).value);

class B {
  x = 1;
  constructor(o: any = this.x + 1) {
    (this as any).v = o;
  }
}
console.log((new B() as any).v, (new B(7) as any).v);

class C {
  constructor(o: any = this) {
    (this as any).same = o === this;
  }
}
console.log((new C() as any).same);
