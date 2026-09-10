// A static method with DECLARED parameters at a spread call site.
// The wrap pass (`spread_callee_wrap`) hands a declined spread site's
// callee to the runtime lane as its `__forward_` relay, and kept every
// `__`-prefixed callee out — the factories and `__cm_` chains have
// receivers or factory semantics a relay would lose. A static
// method's `__sm_` body on the head-less tier has neither: its relay
// is the plain-fn relay, whose adapter enters the body with the true
// argc / argv (argv face) or carries the true count into the hidden
// slot (a length-only body). Without the wrap the site reached the
// runtime lane bare and died at the callee boxing; the zero-parameter
// twin (`static m() { … }`) rode the direct feed all along. Sync and
// generator statics, a class expression, typed and untyped params,
// the escaped value called with its own spread.
const arr = [2, 3];
class C {
  static m(a) { return [a, arguments.length, arguments[3]]; }
  static *g(a) { return [a, arguments.length, arguments[3]]; }
  static t(a: number, b: string) { return [a, b, arguments.length]; }
  static two(a, b) { return [a, b, arguments.length]; }
}
console.log(C.m(42, ...[1], ...arr), C.m(...arr), C.m(7));
var ref = C.m;
console.log(ref(1, ...arr), (C.m as any)(5, 6));
console.log(C.g(42, ...[1], ...arr).next().value, C.g(...arr).next().value);
console.log(C.t(1, "x", ...arr), C.t(...([9, "q"] as any[])));
console.log(C.two(...arr), C.two(1, ...arr), C.two(1, 2, ...arr));
const K = class { static m(a) { return [a, arguments.length]; } };
console.log(K.m(1, ...arr), K.m(...arr));
