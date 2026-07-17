// RFC 20260717-class-first-class-value knife B cut 1 — class methods
// are own properties of C.prototype: gOPD sees the §10.2.10
// {writable, non-enumerable, configurable} descriptor with a
// function-typed value, prototype-receiver calls dispatch through
// the reified cell, and enumeration surfaces stay method-free.
class C {
  method() {
    return 1;
  }
  method2() {
    return 3;
  }
}
const d = Object.getOwnPropertyDescriptor(C.prototype, "method");
console.log(d.configurable, d.enumerable, d.writable);
console.log(typeof d.value);
console.log(C.prototype.method());
console.log(new C().method(), new C().method2());
console.log(Object.keys(C.prototype).length);
class Q2 {
  m() {
    return Q2;
  }
}
console.log(Q2.prototype.m() === Q2);
