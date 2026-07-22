// L3b static-field-reflect (2026-07-22) — class static fields are
// real own entries on the class object: gOPD answers the §7.3.6
// data triple and any-lane member reads see the value ("static" is
// a valid static field name per t262 static-as-valid-*).
class C {
  static static = "test262";
  static num = 42;
}
console.log(C.static, C.num);
const c: any = C;
console.log(c.static, c.num);
const d1: any = Object.getOwnPropertyDescriptor(C, "static");
console.log(d1.value, d1.writable, d1.enumerable, d1.configurable);
const d2: any = Object.getOwnPropertyDescriptor(C, "num");
console.log(d2.value, d2.writable, d2.enumerable, d2.configurable);
class D {
  static = "inst";
}
const d: any = new D();
console.log(d.static);
