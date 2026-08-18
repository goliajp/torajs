// call / apply / bind on a builtin CONSTRUCTOR value (t262
// Function/prototype/bind/15.3.4.5-2-* family) — the bare namespace
// ident lowers to the interned ctor cell, whose boxed entry now
// runs the per-family ctor-as-function conversion: Number =
// ToNumber, String = the display coercion (empty string on no
// args), Boolean = ToBoolean, Object = ToObject, Array = the
// §23.1.1.1 length form, Date = the current-time string (§21.4.2
// ignores its arguments). Families without a callable face
// (Promise, Map, ...) raise the catchable TypeError.
const bnc: any = Number.bind(null);
console.log(bnc(42));
const bsc: any = String.bind(null);
console.log(bsc(123), bsc() === "");
const bbc: any = Boolean.bind(null);
console.log(bbc(1), bbc());
const boc: any = Object.bind(null);
console.log(typeof boc(null));
const bac: any = Array.bind(null);
const a = bac(42);
console.log(a.length);
const bdc: any = Date.bind(null);
console.log(typeof bdc(0, 0, 0));
console.log(Number.call(null, "7"));
console.log(String.call(null, 42));
console.log(Number.apply(null, [8]));
try {
  Promise.call(null, () => {});
} catch (e) {
  console.log("threw", (e as any).constructor.name);
}
try {
  Map.bind(null)();
} catch (e) {
  console.log("threw2", (e as any).constructor.name);
}
console.log("done");
