// RFC 20260802 residue fix — an un-annotated setter param is `any`
// per TS inference; the None-ann Type::Void sig used to fail the
// boxed-adapter gate and silently drop the AccessorPair's set face
// (gOPD .set answered undefined, keyed writes threw "readonly").
//
// p1: plain-ident static accessor — descriptor faces + keyed write.
var s1;
class C1 {
  static get sk() { return "g"; }
  static set sk(param) { s1 = param; }
}
const d = Object.getOwnPropertyDescriptor(C1, "sk");
console.log(typeof d?.get, typeof d?.set);
(C1 as any)["sk"] = "w";
console.log(s1);

// p2: numeric-literal static accessor (test262
// accessor-name-static/literal-numeric-hex shape) — the canonical
// "16" key reads and writes through the class object.
var s2;
class C2 {
  static get 0x10() { return "get string"; }
  static set 0x10(param) { s2 = param; }
}
console.log((C2 as any)["16"]);
(C2 as any)["16"] = "set string";
console.log(s2);

// p3: un-annotated INSTANCE setter through the prototype keyed
// write (the instance-emit mirror of the same dropout).
var s3;
class C3 {
  get g() { return 1; }
  set g(v) { s3 = v; }
}
(C3.prototype as any)["g"] = 9;
console.log(s3);
