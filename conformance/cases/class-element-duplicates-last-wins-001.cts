// §15.7.14 ClassDefinitionEvaluation defines class elements one at a
// time on the same key: a later definition replaces an earlier one, a
// later getter keeps the pair's setter, and a method replaces an
// accessor pair outright (Script goal; test262
// computed-property-names/class/accessor/*-duplicates).
var calls = [];
class C {
  get a() { throw new Error("first a getter is unreachable"); }
  get ['a']() { return 'A'; }
  set b(_) { throw new Error("first b setter is unreachable"); }
  set ['b'](v) { calls.push("b=" + v); }
  get c() { return 'c-get-1'; }
  set c(v) { calls.push("c=" + v); }
  get ['c']() { return 'c-get-2'; }
  m() { return 1; }
  m() { return 2; }
  get d() { return 'd-get'; }
  d() { return 'd-method'; }
  static get s() { return 's1'; }
  static get ['s']() { return 's2'; }
  static k() { return 'k1'; }
  static ['k']() { return 'k2'; }
}
var o = new C();
o.b = 'B';
o.c = 'V';
console.log(o.a, o.c, o.m(), o.d(), C.s, C.k(), calls.join(","));
var desc = Object.getOwnPropertyDescriptor(C.prototype, 'c');
console.log(typeof desc.get, typeof desc.set);
// Fields: every initializer runs in order; the key is one slot.
var x = [];
class F {
  y = (x.push("a"), "old");
  ["y"] = (x.push("b"), "mid");
  y = (x.push("c"), "same");
  static f = 'test';
  static f = this.f + '262';
}
var fo = new F();
console.log(fo.y, x.join(","), F.f, Object.prototype.hasOwnProperty.call(F.prototype, "y"));
