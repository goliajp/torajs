// 398-02 — a static method reached through the any lane carries the
// RECEIVER class: a static face's mono body resolved `this` to the
// declaring class at compile time, so a member call on the class
// object must ride the `__smany_` twin with the receiver
// (§13.3.6.2). The inherited spelling resolves through the
// [[Prototype]] chain; the own spelling through the dynobj entry.

class Base {
  q: any;
  constructor(q: any) {
    this.q = q;
  }
  static make(q: any): any {
    return new (this as any)(q);
  }
}
class Sub extends Base {}

// inherited static through an any binding
const X: any = Sub;
console.log(X.make(6) instanceof Sub);

// inherited static through `as any`
class B2 {
  static who(): any {
    return (this as any).name;
  }
}
class S2 extends B2 {}
console.log((S2 as any).who());

// dynamic key resolves the same twin channel
const k: any = "make";
console.log(X[k](7) instanceof Sub);

// own static on the declaring class keeps its answer
const B: any = Base;
console.log(B.make(5) instanceof Base);

// .call keeps its answer (the verdict this arm mirrors)
console.log((S2 as any).who.call(S2));
