// §15.7.14: every static accessor is defined before any static field initializer runs, and
// the field's DefineField replaces a same-named accessor with a data property — in either
// source order. Inside the initializers up to the field's own, the name is still the accessor.
class A { static get x() { return "acc"; } static x = "field"; }
console.log(A.x, Object.getOwnPropertyDescriptor(A, "x").writable, Object.keys(A).join(","));
class B { static x = "field"; static get x() { return "acc"; } }
console.log(B.x, typeof Object.getOwnPropertyDescriptor(B, "x").get);
var seenC = "unset";
class C { static set y(v) { seenC = v; } static y = 3; }
C.y = 7;
console.log(C.y, seenC);
class D { static z = 1; static get z() { return 9; } static set z(v) {} }
D.z = 5;
console.log(D.z);
var blockE = "unset";
class E {
  static get x() { return "acc"; }
  static early = this.x;
  static x = "field";
  static late = this.x;
  static { blockE = E.x; }
}
console.log(E.early, E.x, E.late, blockE);
