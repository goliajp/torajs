// A numeric literal property name is spelled by Number::toString (§7.1.19 →
// §7.1.17), not by its source text: 0.0000001 names "1e-7", 1e21 names
// "1e+21", 0x10 names "16" — in class members and object literals alike.
class C {
  get 0.0000001() { return "g"; }
  set 0.0000001(p: any) { console.log("set", p); }
  static get 1e21() { return "s"; }
  0.000001() { return "m"; }
}
const P: any = C.prototype;
console.log(P["1e-7"], Object.getOwnPropertyNames(C.prototype), (C as any)["1e+21"], P["0.000001"]);
P["1e-7"] = 5;
const o: any = { 0.0000001: 1, 1e21: 2, 0x10: 3 };
console.log(Object.keys(o));
