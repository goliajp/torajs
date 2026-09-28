// a class method taken as a value may be called bare with arguments
// missing; inside the body the parameter then reads as undefined
// wherever the program asks (typeof, a property read, arithmetic)
class D {
  ty(s: string) { return typeof s; }
  len(s: string) { return s.length; }
  add(s: string) { return s + 1; }
  tyn(x: number) { return typeof x; }
  addn(x: number) { return x + 1; }
}
const d = new D();
const ty = d.ty as any;
const len = d.len as any;
const add = d.add as any;
const tyn = d.tyn as any;
const addn = d.addn as any;
console.log(ty("x"), ty(), tyn(3), tyn());
console.log(len("abc"), add("a"), add(), addn(2), addn());
try { console.log(len()); } catch (e) { console.log(e instanceof TypeError); }
console.log(d.ty("y"), d.len("yy"), d.add("y"), d.tyn(1), d.addn(1));
