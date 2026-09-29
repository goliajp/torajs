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
  static twice(k: number) { return k * 2; }
  static isU(k: number) { return k === undefined; }
}
const e = new E();
console.log(e.flip.call(e), e.ty.call(e), e.ty.call(e, false));
console.log(E.ty.call(null), E.ty.call(null, true));
const f = e.flip as any;
const t = e.ty as any;
console.log(f(), t(), t(true), f(false));
const tw = E.twice as any;
const iu = E.isU as any;
console.log(tw(), tw(21), iu(), iu(3));
console.log(e.flip(false), E.twice(4), E.isU(0));
