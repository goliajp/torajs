// a class method with a boolean parameter taken as a value and called
// without a receiver: an argument arrives as itself, a missing one as
// undefined (the receiver-polymorphic twin hosts the bare call)
class D {
  flip(b: boolean) { return !b; }
  ty(b: boolean) { return typeof b; }
  isU(b: boolean) { return b === undefined; }
  static k(b: boolean) { return b ? 1 : 0; }
  static ty(b: boolean) { return typeof b; }
}
const d = new D();
const f = d.flip;
const t = d.ty;
const u = d.isU;
const k = D.k;
const st = D.ty;
console.log(f(true), f(false), t(true), u(false), k(true), k(false), st(false));
const f2 = d.flip as any;
const t2 = d.ty as any;
const u2 = d.isU as any;
const k2 = D.k as any;
const st2 = D.ty as any;
console.log(f2(), t2(), u2(), k2(), st2());
console.log(d.flip(true), d.ty(false), D.k(true), D.ty(true));
