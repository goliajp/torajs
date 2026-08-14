// §15.7.14 — the receiver of `Sub.make()` is the `Sub` constructor
// object, even when `make` is inherited. The static-inheritance wedge
// aliases `Sub.make` onto the owner's binding, so the call used to
// become a direct `__sm_Base__make(3)`: no receiver channel, and `this`
// already minted to the DECLARING class name. A static factory then
// silently built a `Base`. Calls whose owner body reads `this` route to
// the receiver-polymorphic twin with the accessing class as receiver.

class Base {
  n: number;
  constructor(q: number) { this.n = q }
  static make(q: number): any { return new (this as any)(q) }
  static who(): any { return (this as any).name }
  static bias: number = 0;
  static biased(a: number): any { return a + ((this as any).bias as number) }
  static readBias(): any { return (this as any).bias }
  // this-free: runs identically under any receiver, keeps direct dispatch
  static plain(q: number): number { return q + 1 }
}
class Mid extends Base {}
class Sub extends Mid {
  static bias: number = 100;
}
class Own extends Base {
  static make(q: number): any { return new (this as any)(q + 1000) }
}

const a: any = Sub.make(3);
console.log(a.n, a instanceof Sub, a instanceof Mid, a instanceof Base, a.constructor === Sub);

const m: any = Mid.make(4);
console.log(m.n, m instanceof Mid, m.constructor === Mid);

const o: any = Base.make(5);
console.log(o.n, o instanceof Base, o.constructor === Base);

// an own static shadows the inherited one: receiver is the declaring class
const w: any = Own.make(6);
console.log(w.n, w instanceof Own);

// `this.name` through a two-link chain
console.log(Base.who(), Mid.who(), Sub.who());

// a static field read through `this` follows the receiver's own binding
console.log(Base.readBias(), Sub.readBias(), Sub.bias);
console.log(Base.biased(1), Mid.biased(1), Sub.biased(1));

// this-free inherited static
console.log(Sub.plain(9), Mid.plain(9), Base.plain(9));

// routed calls nested inside other expressions
console.log([Sub.make(7).n, Mid.make(8).n].join(","));
console.log(Sub.who() + "/" + Mid.who());

// read as a value: an unbound function per §15.7.14, receiver decided later
const f: any = Sub.who;
console.log(typeof f);

// the reflective rebind path keeps working alongside the direct one
console.log((Base as any).who.call(Sub), (Base as any).make.call(Sub, 9).n);
