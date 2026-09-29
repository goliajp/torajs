// a boolean / integer parameter method reached through a value with
// the right receiver but a missing argument: the adapter sees the
// undefined box and hands the call to the receiver-polymorphic twin
// instead of unboxing undefined to false / 0; with every argument
// present the typed mono body runs
class E {
  n = 1;
  flip(b: boolean) { return !b; }
  ty(b: boolean) { return typeof b; }
  static ty(b: boolean) { return typeof b; }
}
const e = new E();
console.log(e.flip.call(e), e.ty.call(e), e.ty.call(e, false));
console.log(E.ty.call(null), E.ty.call(null, true));
const f = e.flip as any;
const t = e.ty as any;
console.log(f(), t(), t(true), f(false));
console.log(e.flip(false), E.ty(true));
