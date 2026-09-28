// a class method with a string parameter taken as a value and called
// without a receiver: an argument arrives as itself, a missing one as
// undefined (the string slot keeps its undefined sentinel)
class D {
  greet(s: string) { return "hi " + s; }
  tpl(s: string) { return `<${s}>`; }
  isU(s: string) { return s === undefined; }
  id(s: string) { return s; }
  static k(s: string) { return s + "!"; }
}
const d = new D();
const f = d.greet;
const t = d.tpl;
const u = d.isU;
const i = d.id;
const k = D.k;
console.log(f("x"), t("x"), u("x"), i("x"), k("z"));
const g = d.greet as any;
const t2 = d.tpl as any;
const u2 = d.isU as any;
const i2 = d.id as any;
const k2 = D.k as any;
console.log(g(), t2(), u2(), i2(), k2());
console.log(u2(undefined), u2("undefined"), i2() === undefined, i2() === "undefined");
