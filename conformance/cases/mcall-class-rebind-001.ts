// RFC 20260820-member-call-route 刀 1 — Function.prototype
// call/apply/bind on an INLINE class-instance method read
// (`a.m.call(x)`): the member read lowers to a runtime any cell
// (S2.34 reified class-method cell), so the surfaces any-dispatch
// and the runtime kernel re-binds the thisArg. Inherited methods
// resolve through the prototype chain the same way.
class A {
  v: number = 7;
  m() {
    return (this as any).v;
  }
}
class B extends A {}
const a = new A();
console.log(a.m());
console.log(a.m.call({ v: 42 }));
console.log(a.m.apply({ v: 9 }));
const bf = a.m.bind({ v: 5 });
console.log(bf());
const b = new B();
console.log(b.m.call({ v: 11 }));
