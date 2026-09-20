// The [[Call]] of a builtin constructor read as a VALUE. tr had the
// per-family table — Number is ToNumber, String is the display
// coercion, Object is ToObject, Array is the length form, Date is the
// current time as a string — and three families were missing from it,
// so `const R: any = RegExp; R("a")` said the constructor "cannot be
// called as a function through a first-class value" about three that
// the spec says can.
function t(label: string, f: any): void {
  try {
    console.log(label, "->", String(f()));
  } catch (e: any) {
    console.log(label, "threw", e && e.constructor ? e.constructor.name : "?");
  }
}
// §22.2.4.1 — with NewTarget undefined, a RegExp argument and no
// flags answers the argument itself; everything else is built.
const R: any = RegExp;
t("RegExp('a').source ", () => R("a").source);
t("RegExp('a','g')    ", () => R("a", "g").flags);
const lit = /b/;
t("RegExp(lit)===lit  ", () => R(lit) === lit);
t("RegExp(lit,'g')    ", () => R(lit, "g").flags);
t("RegExp().source    ", () => R().source);
t("RegExp('(')        ", () => R("(").source);
t("new RegExp('a')    ", () => new R("a").source);
// §20.4.1.1 / §21.2.1.1 — these two exist ONLY in the call form.
const Sy: any = Symbol;
t("Symbol('d')        ", () => String(Sy("d")));
t("Symbol()           ", () => String(Sy()));
t("typeof Symbol('d') ", () => typeof Sy("d"));
t("new Symbol('d')    ", () => new Sy("d"));
const Bi: any = BigInt;
t("BigInt(5)          ", () => Bi(5));
t("BigInt('7')        ", () => Bi("7"));
t("BigInt(true)       ", () => Bi(true));
// §21.2.1.1 takes NumberToBigInt, not ToBigInt: a Number is accepted
// and a non-integer is a RangeError, where ToBigInt refuses every
// Number with a TypeError.
t("BigInt(1.5)        ", () => Bi(1.5));
t("typeof BigInt(5)   ", () => typeof Bi(5));
t("new BigInt(5)      ", () => new Bi(5));
