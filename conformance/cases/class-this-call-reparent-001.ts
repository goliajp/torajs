// this.m() inside a method dispatches on the receiver's chain once the
// instance was re-parented; super.m() stays static.
class C {
  run() { return this.m() + ":" + this.tag(); }
  m() { return "C.m"; }
  tag() { return "t"; }
}
class Sub extends C {
  m() { return "Sub.m/" + super.m(); }
}
const c = new C();
const p: any = Object.create(C.prototype);
p.m = function () { return "p.m"; };
Object.setPrototypeOf(c, p);
console.log(c.run());
const s = new Sub();
console.log(s.run());
Object.setPrototypeOf(s, C.prototype);
console.log(s.run());
